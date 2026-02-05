"""Asset helpers implemented via fsspec."""

from __future__ import annotations

import json
import uuid
from typing import TYPE_CHECKING, TypedDict, cast

import ieapp_core

if TYPE_CHECKING:
    import fsspec

from .utils import (
    fs_exists,
    fs_join,
    fs_ls,
    fs_read_json,
    get_fs_and_path,
    run_async,
    split_space_path,
    storage_config_from_root,
    validate_id,
)


class AssetReferencedError(Exception):
    """Raised when attempting to delete an asset that is still in use."""


class AssetMeta(TypedDict):
    """Asset metadata payload."""

    id: str
    name: str
    path: str


def _space_context(
    space_path: str,
    fs: fsspec.AbstractFileSystem | None = None,
) -> tuple[fsspec.AbstractFileSystem, str, str]:
    fs_obj, ws_path = get_fs_and_path(space_path, fs)
    space_id = validate_id(ws_path.rstrip("/").split("/")[-1], "space_id")
    if not fs_exists(fs_obj, ws_path):
        msg = f"Space {space_id} not found"
        raise FileNotFoundError(msg)
    return fs_obj, ws_path, space_id


def save_asset(
    space_path: str,
    data: bytes,
    filename: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> AssetMeta:
    """Persist a binary blob within the space assets directory."""
    _ = uuid
    if fs is not None:
        fs_obj, ws_path, _space_id = _space_context(space_path, fs)
        asset_id = uuid.uuid4().hex
        safe_name = filename or asset_id
        asset_name = f"{asset_id}_{safe_name}"
        asset_path = fs_join(ws_path, "assets", asset_name)
        with fs_obj.open(asset_path, "wb") as handle:
            handle.write(data)
        return {
            "id": asset_id,
            "name": safe_name,
            "path": f"assets/{asset_name}",
        }

    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path, fs)
    return cast(
        "AssetMeta",
        run_async(
            ieapp_core.save_asset,
            config,
            space_id,
            filename or "",
            data,
        ),
    )


def list_assets(
    space_path: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> list[AssetMeta]:
    """Return asset metadata stored in the space."""
    if fs is not None:
        fs_obj, ws_path, _space_id = _space_context(space_path, fs)
        assets_dir = fs_join(ws_path, "assets")
        if not fs_exists(fs_obj, assets_dir):
            return []
        assets: list[AssetMeta] = []
        for entry in fs_obj.ls(assets_dir, detail=False):
            name = entry.split("/")[-1]
            if "_" not in name:
                continue
            asset_id, original = name.split("_", 1)
            assets.append(
                {
                    "id": asset_id,
                    "name": original,
                    "path": f"assets/{name}",
                },
            )
        return assets

    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path, fs)
    return cast(
        "list[AssetMeta]",
        run_async(ieapp_core.list_assets, config, space_id),
    )


def _is_asset_referenced(
    fs_obj: fsspec.AbstractFileSystem,
    ws_path: str,
    asset_id: str,
) -> bool:
    entries_dir = fs_join(ws_path, "entries")
    if not fs_exists(fs_obj, entries_dir):
        return False

    for entry_dir in fs_ls(fs_obj, entries_dir):
        content_path = fs_join(entry_dir, "content.json")
        if not fs_exists(fs_obj, content_path):
            continue
        try:
            content = fs_read_json(fs_obj, content_path)
        except (json.JSONDecodeError, OSError):
            # Ignore unreadable/invalid entry files
            continue
        for asset in content.get("assets", []) or []:
            if asset.get("id") == asset_id:
                return True
    return False


def delete_asset(
    space_path: str,
    asset_id: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> None:
    """Delete an asset if it is not referenced by any entry."""
    validate_id(asset_id, "asset_id")
    if fs is not None:
        fs_obj, ws_path, _space_id = _space_context(space_path, fs)
        if _is_asset_referenced(fs_obj, ws_path, asset_id):
            msg = f"Asset {asset_id} is referenced by a entry"
            raise AssetReferencedError(msg)

        assets_dir = fs_join(ws_path, "assets")
        if not fs_exists(fs_obj, assets_dir):
            raise FileNotFoundError(asset_id)

        deleted = False
        for entry in fs_obj.ls(assets_dir, detail=False):
            name = entry.split("/")[-1]
            if name.startswith(f"{asset_id}_"):
                fs_obj.rm(entry)
                deleted = True
        if not deleted:
            raise FileNotFoundError(asset_id)
        return

    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path, fs)
    try:
        run_async(ieapp_core.delete_asset, config, space_id, asset_id)
    except RuntimeError as exc:
        msg = str(exc)
        if "referenced" in msg:
            raise AssetReferencedError(msg) from exc
        if "not found" in msg:
            raise FileNotFoundError(asset_id) from exc
        raise
