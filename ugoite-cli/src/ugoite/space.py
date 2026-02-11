"""Space management module."""

import base64
import json
import logging
import secrets
import uuid
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import fsspec
import ugoite_core

try:  # pragma: no cover - platform specific
    import fcntl

    # declaring a local annotation to make the type checker happy
    fcntl: Any
except ImportError:  # pragma: no cover - platform specific
    # fcntl is not available on Windows/python distributions such as pypy
    fcntl: Any | None = None

from .utils import (
    fs_exists,
    fs_join,
    fs_read_json,
    fs_write_json,
    get_fs_and_path,
    run_async,
    storage_config_from_root,
    storage_uri_from_root,
    validate_id,
    validate_uuid,
)

logger = logging.getLogger(__name__)

EMPTY_INDEX_DATA = {"entries": {}, "form_stats": {}}
EMPTY_STATS_DATA = {"last_indexed": 0.0, "entry_count": 0, "tag_counts": {}}
MIN_SAMPLE_ENTRY_COUNT = 100


def _resolve_space_paths(
    root_path: str | Path,
    space_id: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
    must_exist: bool = False,
) -> tuple[fsspec.AbstractFileSystem, str, str]:
    """Return filesystem, spaces dir, and space path strings."""
    safe_space_id = validate_id(space_id, "space_id")
    try:
        fs_obj, base_path = get_fs_and_path(root_path, fs)
    except (ImportError, ValueError) as exc:
        msg = "Protocol not supported in current runtime"
        raise NotImplementedError(msg) from exc
    spaces_dir = fs_join(base_path, "spaces")
    space_path = fs_join(spaces_dir, safe_space_id)

    if must_exist and not fs_exists(fs_obj, space_path):
        msg = f"Space {safe_space_id} not found"
        raise FileNotFoundError(msg)

    return fs_obj, spaces_dir, space_path


class SpaceExistsError(Exception):
    """Raised when trying to create a space that already exists."""


@dataclass(frozen=True)
class SampleSpaceOptions:
    """Configuration for sample data generation."""

    scenario: str = "renewable-ops"
    entry_count: int = 5000
    seed: int | None = None


def _append_space_to_global(
    fs: fsspec.AbstractFileSystem,
    global_json_path: str,
    space_id: str,
) -> None:
    """Append a space id to ``global.json`` using fsspec."""
    if not fs_exists(fs, global_json_path):
        return

    try:
        global_data = fs_read_json(fs, global_json_path)
    except (json.JSONDecodeError, OSError):
        global_data = {"spaces": []}

    spaces = global_data.setdefault("spaces", [])
    if space_id in spaces:
        return

    spaces.append(space_id)
    fs_write_json(fs, global_json_path, global_data)


def _ensure_global_json(fs: fsspec.AbstractFileSystem, root_path: str) -> str:
    """Ensure ``global.json`` exists under ``root_path`` using fsspec."""
    global_json_path = fs_join(root_path, "global.json")
    if fs_exists(fs, global_json_path):
        return global_json_path

    protocol = getattr(fs, "protocol", "file") or "file"
    if isinstance(protocol, (list, tuple)):
        protocol = protocol[0]

    now_iso = datetime.now(UTC).isoformat()
    key_id = f"key-{uuid.uuid4().hex}"
    hmac_key = base64.b64encode(secrets.token_bytes(32)).decode("ascii")

    payload = {
        "version": 1,
        "default_storage": f"{protocol}://{root_path}",
        "spaces": [],
        "hmac_key_id": key_id,
        "hmac_key": hmac_key,
        "last_rotation": now_iso,
    }

    fs_write_json(fs, global_json_path, payload, mode=0o600, exclusive=True)
    return global_json_path


def create_space(
    root_path: str | Path,
    space_id: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> None:
    """Create a new space with the required directory structure."""
    safe_space_id = validate_id(space_id, "space_id")
    if str(root_path).startswith("s3://"):
        msg = "Protocol not supported in current runtime"
        raise NotImplementedError(msg)

    config = storage_config_from_root(root_path, fs)
    try:
        run_async(ugoite_core.create_space, config, safe_space_id)
    except (RuntimeError, ValueError) as exc:
        msg = str(exc)
        if "already exists" in msg:
            raise SpaceExistsError(msg) from exc
        if "scheme is not registered" in msg:
            msg_err = f"Protocol not supported: {msg}"
            raise NotImplementedError(msg_err) from exc
        raise
    logger.info("Space %s created successfully", space_id)


def create_sample_space(
    root_path: str | Path,
    space_id: str,
    *,
    options: SampleSpaceOptions | None = None,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Create a new space populated with generated sample data."""
    safe_space_id = validate_id(space_id, "space_id")
    config = storage_config_from_root(root_path, fs)
    resolved = options or SampleSpaceOptions()

    if resolved.entry_count < MIN_SAMPLE_ENTRY_COUNT:
        msg_err = f"entry_count must be >= {MIN_SAMPLE_ENTRY_COUNT}"
        raise ValueError(msg_err)
    if resolved.seed is not None and resolved.seed < 0:
        msg_err = "seed must be >= 0"
        raise ValueError(msg_err)

    try:
        return run_async(
            ugoite_core.create_sample_space,
            config,
            safe_space_id,
            resolved.scenario,
            resolved.entry_count,
            resolved.seed,
        )
    except (RuntimeError, ValueError, OverflowError, TypeError) as exc:
        msg = str(exc)
        if "already exists" in msg:
            raise SpaceExistsError(msg) from exc
        if "unknown sample data scenario" in msg.lower():
            raise ValueError(msg) from exc
        raise


def list_sample_scenarios() -> list[dict[str, Any]]:
    """List available sample-data scenarios."""
    return ugoite_core.list_sample_scenarios()


def create_sample_space_job(
    root_path: str | Path,
    space_id: str,
    *,
    options: SampleSpaceOptions | None = None,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Create a sample-data generation job."""
    safe_space_id = validate_id(space_id, "space_id")
    config = storage_config_from_root(root_path, fs)
    resolved = options or SampleSpaceOptions()

    if resolved.entry_count < MIN_SAMPLE_ENTRY_COUNT:
        msg_err = f"entry_count must be >= {MIN_SAMPLE_ENTRY_COUNT}"
        raise ValueError(msg_err)
    if resolved.seed is not None and resolved.seed < 0:
        msg_err = "seed must be >= 0"
        raise ValueError(msg_err)

    try:
        return run_async(
            ugoite_core.create_sample_space_job,
            config,
            safe_space_id,
            resolved.scenario,
            resolved.entry_count,
            resolved.seed,
        )
    except (RuntimeError, ValueError, OverflowError, TypeError) as exc:
        msg = str(exc)
        if "already exists" in msg:
            raise SpaceExistsError(msg) from exc
        if "unknown sample data scenario" in msg.lower():
            raise ValueError(msg) from exc
        raise


def get_sample_space_job(
    root_path: str | Path,
    job_id: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Get sample-data job status."""
    safe_job_id = validate_uuid(job_id, "job_id")
    config = storage_config_from_root(root_path, fs)
    try:
        return run_async(ugoite_core.get_sample_space_job, config, safe_job_id)
    except RuntimeError as exc:
        msg = str(exc)
        if "not found" in msg.lower():
            raise FileNotFoundError(msg) from exc
        raise


def get_space(
    root_path: str | Path,
    space_id: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Get space metadata.

    Args:
        root_path: The root directory where spaces are stored.
        space_id: The unique identifier for the space.
        fs: Optional filesystem for non-local storage.

    Returns:
        Dictionary containing space metadata.

    Raises:
        FileNotFoundError: If the space does not exist.

    """
    config = storage_config_from_root(root_path, fs)
    try:
        return run_async(ugoite_core.get_space, config, space_id)
    except RuntimeError as exc:
        msg = str(exc)
        if "not found" in msg:
            raise FileNotFoundError(msg) from exc
        raise


def list_spaces(
    root_path: str | Path,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> list[dict[str, Any]]:
    """List all spaces.

    Args:
        root_path: The root directory where spaces are stored.
        fs: Optional filesystem for non-local storage.

    Returns:
        List of space metadata dictionaries.

    """
    if fs is not None:
        fs_obj, base_path = get_fs_and_path(root_path, fs)
        global_path = fs_join(base_path, "global.json")
        if not fs_exists(fs_obj, global_path):
            return []
        try:
            global_data = fs_read_json(fs_obj, global_path)
        except (json.JSONDecodeError, OSError):
            return []
        space_ids = global_data.get("spaces", []) or []
        results: list[dict[str, Any]] = []
        for ws_id in space_ids:
            try:
                results.append(get_space(root_path, str(ws_id), fs=fs_obj))
            except (FileNotFoundError, ValueError):
                continue
        return results

    config = storage_config_from_root(root_path)
    space_ids = run_async(ugoite_core.list_spaces, config)
    results: list[dict[str, Any]] = []
    for ws_id in space_ids:
        try:
            results.append(run_async(ugoite_core.get_space, config, ws_id))
        except RuntimeError as exc:
            logger.warning("Failed to read space meta %s: %s", ws_id, exc)
            continue
    return results


def space_path(
    root_path: str | Path,
    space_id: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
    must_exist: bool = False,
) -> str:
    """Public helper returning the absolute space path string."""
    if must_exist:
        _ = get_space(root_path, space_id, fs=fs)
    root_uri = storage_uri_from_root(root_path, fs)
    if root_uri.startswith("fs://") and "://" not in str(root_path):
        return fs_join(str(root_path), "spaces", space_id)
    return f"{root_uri.rstrip('/')}/spaces/{space_id}"


def patch_space(
    root_path: str | Path,
    space_id: str,
    *,
    patch: dict[str, Any] | None = None,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Update space metadata and settings using fsspec.

    The `patch` dict may contain keys: ``name``, ``storage_config``, and ``settings``.
    """
    config = storage_config_from_root(root_path, fs)
    patch_payload = patch or {}
    try:
        return run_async(
            ugoite_core.patch_space,
            config,
            space_id,
            json.dumps(patch_payload),
        )
    except RuntimeError as exc:
        msg = str(exc)
        if "not found" in msg:
            raise FileNotFoundError(msg) from exc
        raise


def test_storage_connection(storage_config: dict[str, Any]) -> dict[str, object]:
    """Validate storage connector payload (stub for now)."""
    return run_async(ugoite_core.test_storage_connection, storage_config)
