"""Attachment helpers implemented via fsspec."""

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
    split_workspace_path,
    storage_config_from_root,
    validate_id,
)


class AttachmentReferencedError(Exception):
    """Raised when attempting to delete an attachment that is still in use."""


class AttachmentMeta(TypedDict):
    """Attachment metadata payload."""

    id: str
    name: str
    path: str


def _workspace_context(
    workspace_path: str,
    fs: fsspec.AbstractFileSystem | None = None,
) -> tuple[fsspec.AbstractFileSystem, str, str]:
    fs_obj, ws_path = get_fs_and_path(workspace_path, fs)
    workspace_id = validate_id(ws_path.rstrip("/").split("/")[-1], "workspace_id")
    if not fs_exists(fs_obj, ws_path):
        msg = f"Workspace {workspace_id} not found"
        raise FileNotFoundError(msg)
    return fs_obj, ws_path, workspace_id


def save_attachment(
    workspace_path: str,
    data: bytes,
    filename: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> AttachmentMeta:
    """Persist a binary blob within the workspace attachments directory."""
    _ = uuid
    if fs is not None:
        fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path, fs)
        attachment_id = uuid.uuid4().hex
        safe_name = filename or attachment_id
        attachment_name = f"{attachment_id}_{safe_name}"
        attachment_path = fs_join(ws_path, "attachments", attachment_name)
        with fs_obj.open(attachment_path, "wb") as handle:
            handle.write(data)
        return {
            "id": attachment_id,
            "name": safe_name,
            "path": f"attachments/{attachment_name}",
        }

    root_path, workspace_id = split_workspace_path(workspace_path)
    config = storage_config_from_root(root_path, fs)
    return cast(
        "AttachmentMeta",
        run_async(
            ieapp_core.save_attachment,
            config,
            workspace_id,
            filename or "",
            data,
        ),
    )


def list_attachments(
    workspace_path: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> list[AttachmentMeta]:
    """Return attachment metadata stored in the workspace."""
    if fs is not None:
        fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path, fs)
        attachments_dir = fs_join(ws_path, "attachments")
        if not fs_exists(fs_obj, attachments_dir):
            return []
        attachments: list[AttachmentMeta] = []
        for entry in fs_obj.ls(attachments_dir, detail=False):
            name = entry.split("/")[-1]
            if "_" not in name:
                continue
            attachment_id, original = name.split("_", 1)
            attachments.append(
                {
                    "id": attachment_id,
                    "name": original,
                    "path": f"attachments/{name}",
                },
            )
        return attachments

    root_path, workspace_id = split_workspace_path(workspace_path)
    config = storage_config_from_root(root_path, fs)
    return cast(
        "list[AttachmentMeta]",
        run_async(ieapp_core.list_attachments, config, workspace_id),
    )


def _is_attachment_referenced(
    fs_obj: fsspec.AbstractFileSystem,
    ws_path: str,
    attachment_id: str,
) -> bool:
    notes_dir = fs_join(ws_path, "notes")
    if not fs_exists(fs_obj, notes_dir):
        return False

    for note_dir in fs_ls(fs_obj, notes_dir):
        content_path = fs_join(note_dir, "content.json")
        if not fs_exists(fs_obj, content_path):
            continue
        try:
            content = fs_read_json(fs_obj, content_path)
        except (json.JSONDecodeError, OSError):
            # Ignore unreadable/invalid note files
            continue
        for attachment in content.get("attachments", []) or []:
            if attachment.get("id") == attachment_id:
                return True
    return False


def delete_attachment(
    workspace_path: str,
    attachment_id: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> None:
    """Delete an attachment if it is not referenced by any note."""
    validate_id(attachment_id, "attachment_id")
    if fs is not None:
        fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path, fs)
        if _is_attachment_referenced(fs_obj, ws_path, attachment_id):
            msg = f"Attachment {attachment_id} is referenced by a note"
            raise AttachmentReferencedError(msg)

        attachments_dir = fs_join(ws_path, "attachments")
        if not fs_exists(fs_obj, attachments_dir):
            raise FileNotFoundError(attachment_id)

        deleted = False
        for entry in fs_obj.ls(attachments_dir, detail=False):
            name = entry.split("/")[-1]
            if name.startswith(f"{attachment_id}_"):
                fs_obj.rm(entry)
                deleted = True
        if not deleted:
            raise FileNotFoundError(attachment_id)
        return

    root_path, workspace_id = split_workspace_path(workspace_path)
    config = storage_config_from_root(root_path, fs)
    try:
        run_async(ieapp_core.delete_attachment, config, workspace_id, attachment_id)
    except RuntimeError as exc:
        msg = str(exc)
        if "referenced" in msg:
            raise AttachmentReferencedError(msg) from exc
        if "not found" in msg:
            raise FileNotFoundError(attachment_id) from exc
        raise
