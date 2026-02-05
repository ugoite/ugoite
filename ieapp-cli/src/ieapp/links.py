"""Link management helpers using fsspec-backed storage."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any, cast

if TYPE_CHECKING:
    import fsspec

import ieapp_core

from .utils import (
    fs_exists,
    fs_read_json,
    get_fs_and_path,
    run_async,
    split_space_path,
    storage_config_from_root,
    validate_id,
)


def _space_context(
    space_path: str,
    fs: fsspec.AbstractFileSystem | None = None,
) -> tuple[fsspec.AbstractFileSystem, str]:
    fs_obj, ws_path = get_fs_and_path(space_path, fs)
    if not fs_exists(fs_obj, ws_path):
        msg = f"Space not found: {space_path}"
        raise FileNotFoundError(msg)
    return fs_obj, ws_path


def _load_meta(fs_obj: fsspec.AbstractFileSystem, meta_path: str) -> dict[str, Any]:
    return fs_read_json(fs_obj, meta_path)


def create_link(
    space_path: str,
    **kwargs: object,
) -> dict[str, object]:
    """Create a bi-directional link between two entries and persist metadata.

    Accepts keyword args: ``source``, ``target``, ``kind``, ``link_id``, and
    optional ``fs``. This form keeps the runtime call-site flexible while
    avoiding local lint complaints about too many explicit parameters.
    """
    # Convert to expected types for downstream APIs
    source = str(kwargs["source"])
    target = str(kwargs["target"])
    kind = str(kwargs["kind"])
    link_id = str(kwargs["link_id"])
    fs = kwargs.get("fs")
    fs = cast("fsspec.AbstractFileSystem | None", fs)

    validate_id(source, "source")
    validate_id(target, "target")
    validate_id(link_id, "link_id")

    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path, fs)
    try:
        return run_async(
            ieapp_core.create_link,
            config,
            space_id,
            source,
            target,
            kind,
            link_id,
        )
    except RuntimeError as exc:
        msg = str(exc)
        if "not found" in msg:
            raise FileNotFoundError(msg) from exc
        raise


def list_links(
    space_path: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> list[dict[str, Any]]:
    """Return deduplicated links in a space."""
    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path, fs)
    return run_async(ieapp_core.list_links, config, space_id)


def delete_link(
    space_path: str,
    link_id: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> None:
    """Delete a link and remove it from all entries in the space."""
    validate_id(link_id, "link_id")
    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path, fs)
    try:
        run_async(ieapp_core.delete_link, config, space_id, link_id)
    except RuntimeError as exc:
        msg = str(exc)
        if "not found" in msg:
            raise FileNotFoundError(msg) from exc
        raise
