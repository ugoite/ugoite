"""Utility functions for ieapp."""

import asyncio
import concurrent.futures
import contextlib
import json
import os
import posixpath
import re
from collections.abc import Awaitable, Callable
from collections.abc import Awaitable as AwaitableABC
from pathlib import Path
from typing import Any, ParamSpec, TypeVar, cast, overload

import fsspec
from fsspec.core import url_to_fs


def resolve_existing_path(base: Path, *parts: str) -> Path:
    """Securely resolve a path to an EXISTING file or directory.

    This function avoids constructing paths from user input by iterating
    directory contents to find matches. This breaks the taint chain for
    static analysis tools like CodeQL.

    Args:
        base: The base directory to start from.
        *parts: Path components to traverse.

    Returns:
        The resolved Path object found in the filesystem.

    Raises:
        FileNotFoundError: If any component does not exist.
        NotADirectoryError: If a component is not a directory when it should be.

    """
    current = base.resolve()
    if not current.exists():
        msg = f"Base path {current} does not exist"
        raise FileNotFoundError(msg)

    for part in parts:
        if not current.is_dir():
            msg = f"{current} is not a directory"
            raise NotADirectoryError(msg)

        # Iterate over directory contents to find the matching child.
        # This ensures the returned path comes from the OS, not user input.
        found = False
        for child in current.iterdir():
            if child.name == part:
                current = child
                found = True
                break

        if not found:
            msg = f"Component {part} not found in {current}"
            raise FileNotFoundError(msg)

    return current


def join_secure_path(base: Path, name: str) -> Path:
    """Securely construct a path for a NEW file or directory.

    This function strictly validates the name component to ensure it
    contains only safe characters, preventing path traversal.

    Args:
        base: The base directory (must be trusted/resolved).
        name: The name of the new file or directory.

    Returns:
        The constructed Path object.

    Raises:
        ValueError: If the name contains invalid characters.

    """
    # Strict allowlist for new filenames
    if not re.match(r"^[a-zA-Z0-9_.-]+$", name):
        msg = (
            f"Invalid filename: {name}. Allowed: alphanumeric, dot, hyphen, underscore."
        )
        raise ValueError(msg)

    # Explicitly reject traversal indicators even if regex somehow missed them
    if ".." in name or "/" in name or "\\" in name:
        msg = f"Invalid filename: {name}"
        raise ValueError(msg)

    return base / name


def validate_id(identifier: str, name: str) -> str:
    """Validate that an identifier contains only safe characters.

    Returns the sanitized value. This function acts as a security
    sanitizer - it validates the input and returns a safe copy.

    Args:
        identifier: The string to validate.
        name: The name of the field (for error messages).

    Returns:
        The validated identifier (a safe copy).

    Raises:
        ValueError: If the identifier contains invalid characters.

    """
    if not identifier or not re.match(r"^[a-zA-Z0-9_-]+$", identifier):
        msg = (
            f"Invalid {name}: {identifier}. "
            "Must be alphanumeric, hyphens, or underscores."
        )
        raise ValueError(msg)
    # Return a sanitized copy - this breaks the taint chain
    return str(identifier)


def write_json_secure(
    path: str | Path,
    payload: dict[str, Any],
    mode: int = 0o600,
    *,
    exclusive: bool = False,
) -> None:
    """Write JSON to ``path`` while applying permissions atomically.

    Args:
        path: Target file path.
        payload: JSON-serializable dictionary.
        mode: Permission bits applied at creation.
        exclusive: When True, use ``O_EXCL`` to avoid clobbering existing files.

    """
    flags = os.O_WRONLY | os.O_CREAT
    if exclusive:
        flags |= os.O_EXCL
    else:
        flags |= os.O_TRUNC

    fd = os.open(str(path), flags, mode)
    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2)


# ---------------------------------------------------------------------------
# fsspec helpers
# ---------------------------------------------------------------------------


def get_fs_and_path(
    base_path: str | Path,
    fs: fsspec.AbstractFileSystem | None = None,
) -> tuple[fsspec.AbstractFileSystem, str]:
    """Return an fsspec filesystem and a normalized base path string.

    When ``fs`` is provided, the base path is normalized as a POSIX string
    without trailing slashes. Otherwise ``url_to_fs`` is used to infer the
    filesystem from the path or URI.
    """
    if fs is not None:
        return fs, str(base_path).rstrip("/") or "/"

    inferred_fs, path = url_to_fs(str(base_path))
    return inferred_fs, str(path).rstrip("/") or "/"


def fs_join(base: str, *parts: str) -> str:
    """Join path parts using POSIX semantics (suitable for fsspec)."""
    return posixpath.join(base, *parts)


def fs_makedirs(
    fs: fsspec.AbstractFileSystem,
    path: str,
    *,
    mode: int = 0o700,
    exist_ok: bool = True,
) -> None:
    """Create a directory and apply permissions when supported."""
    fs.makedirs(path, exist_ok=exist_ok)
    if hasattr(fs, "chmod"):
        with contextlib.suppress(Exception):
            cast("Any", fs).chmod(path, mode)


def fs_write_json(
    fs: fsspec.AbstractFileSystem,
    path: str,
    payload: dict[str, Any],
    *,
    mode: int = 0o600,
    exclusive: bool = False,
) -> None:
    """Write JSON via fsspec, optionally enforcing exclusivity."""
    if exclusive and fs.exists(path):
        raise FileExistsError(path)

    with fs.open(path, "w") as handle:
        json.dump(payload, handle, indent=2)
    if hasattr(fs, "chmod"):
        with contextlib.suppress(Exception):
            cast("Any", fs).chmod(path, mode)


_P = ParamSpec("_P")
_T = TypeVar("_T")


# ---------------------------------------------------------------------------
# ieapp-core bridge helpers
# ---------------------------------------------------------------------------


@overload
def run_async(
    awaitable_or_factory: Awaitable[_T],
    *args: _P.args,
    **kwargs: _P.kwargs,
) -> _T: ...


@overload
def run_async(
    awaitable_or_factory: Callable[_P, Awaitable[_T]],
    *args: _P.args,
    **kwargs: _P.kwargs,
) -> _T: ...


@overload
def run_async(
    awaitable_or_factory: Callable[_P, _T],
    *args: _P.args,
    **kwargs: _P.kwargs,
) -> _T: ...


def run_async(
    awaitable_or_factory: Awaitable[_T] | Callable[_P, _T | Awaitable[_T]],
    *args: _P.args,
    **kwargs: _P.kwargs,
) -> _T:
    """Run an async coroutine from sync code.

    Accepts either an awaitable or a callable that returns an awaitable.
    """

    async def _runner() -> _T:
        if isinstance(awaitable_or_factory, AwaitableABC):
            return await cast("Awaitable[_T]", awaitable_or_factory)
        if callable(awaitable_or_factory):
            result = awaitable_or_factory(*args, **kwargs)
            if isinstance(result, AwaitableABC):
                return await cast("Awaitable[_T]", result)
            return cast("_T", result)
        msg = "Expected awaitable or awaitable factory"
        raise TypeError(msg)

    try:
        asyncio.get_running_loop()
    except RuntimeError:
        return asyncio.run(_runner())

    def _run_in_new_loop() -> _T:
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        try:
            return loop.run_until_complete(_runner())
        finally:
            loop.close()

    with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(_run_in_new_loop)
        return future.result()


def storage_uri_from_root(
    root_path: str | Path,
    fs: fsspec.AbstractFileSystem | None = None,
) -> str:
    """Return an OpenDAL-compatible storage URI for root_path."""
    root_str = str(root_path)
    if "://" in root_str:
        return root_str

    fs_obj, base_path = get_fs_and_path(root_str, fs)
    protocol = getattr(fs_obj, "protocol", "file") or "file"
    if isinstance(protocol, (list, tuple)):
        protocol = protocol[0]

    if protocol in {"file", "fs"}:
        return f"fs://{base_path}"
    if protocol == "memory":
        return f"memory://{base_path}"

    msg = "Protocol not supported in current runtime"
    raise NotImplementedError(msg)


def storage_config_from_root(
    root_path: str | Path,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, str]:
    """Build ieapp-core storage_config from root_path."""
    return {"uri": storage_uri_from_root(root_path, fs)}


def split_workspace_path(workspace_path: str | Path) -> tuple[str, str]:
    """Split workspace_path into (root_path, workspace_id)."""
    path_str = str(workspace_path)
    if "/workspaces/" in path_str:
        root, ws_id = path_str.split("/workspaces/", 1)
        return root, ws_id.strip("/")
    parts = path_str.rstrip("/").split("/")
    if not parts:
        msg = f"Invalid workspace path: {workspace_path}"
        raise ValueError(msg)
    ws_id = parts[-1]
    root = "/".join(parts[:-1])
    return root, ws_id


def fs_read_json(fs: fsspec.AbstractFileSystem, path: str) -> dict[str, Any]:
    """Read JSON content via fsspec."""
    with fs.open(path, "r") as handle:
        return json.load(handle)


def fs_exists(fs: fsspec.AbstractFileSystem, path: str) -> bool:
    """Return True if the given path exists on the filesystem."""
    return bool(fs.exists(path))


def fs_isdir(fs: fsspec.AbstractFileSystem, path: str) -> bool:
    """Return True when path exists and is a directory (best-effort)."""
    try:
        info = fs.info(path)
    except FileNotFoundError:
        return False
    return info.get("type") == "directory"


def fs_ls(fs: fsspec.AbstractFileSystem, path: str) -> list[str]:
    """List directory entries returning path strings."""
    entries = fs.ls(path, detail=False)
    return [str(entry) for entry in entries]
