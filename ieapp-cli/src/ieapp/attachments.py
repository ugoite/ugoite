"""Attachment helpers implemented via fsspec."""

from __future__ import annotations

import json
import uuid
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import fsspec

from .utils import (
    fs_exists,
    fs_join,
    fs_ls,
    fs_makedirs,
    fs_read_json,
    get_fs_and_path,
    validate_id,
)


class AttachmentReferencedError(Exception):
    """Raised when attempting to delete an attachment that is still in use."""


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
) -> dict[str, str]:
    """Persist a binary blob within the workspace attachments directory."""
    fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path, fs)
    attachments_dir = fs_join(ws_path, "attachments")
    fs_makedirs(fs_obj, attachments_dir, exist_ok=True)

    attachment_id = uuid.uuid4().hex
    safe_name = filename or attachment_id
    relative_path = f"attachments/{attachment_id}_{safe_name}"
    destination = fs_join(ws_path, relative_path)

    with fs_obj.open(destination, "wb") as handle:
        handle.write(data)

    return {"id": attachment_id, "name": safe_name, "path": relative_path}


def list_attachments(
    workspace_path: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> list[dict[str, str]]:
    """Return attachment metadata stored in the workspace."""
    fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path, fs)
    attachments_dir = fs_join(ws_path, "attachments")
    if not fs_exists(fs_obj, attachments_dir):
        return []

    items: list[dict[str, str]] = []
    try:
        entries = fs_obj.ls(attachments_dir, detail=False)
    except FileNotFoundError:
        return []

    for entry in entries:
        filename = str(entry).split("/")[-1]
        if "_" not in filename:
            continue
        attachment_id, name = filename.split("_", 1)
        items.append(
            {"id": attachment_id, "name": name, "path": f"attachments/{filename}"},
        )

    return items


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
    fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path, fs)

    if _is_attachment_referenced(fs_obj, ws_path, attachment_id):
        msg = f"Attachment {attachment_id} is referenced by a note"
        raise AttachmentReferencedError(msg)

    attachments_dir = fs_join(ws_path, "attachments")
    if not fs_exists(fs_obj, attachments_dir):
        raise FileNotFoundError(attachment_id)

    deleted = False
    try:
        entries = fs_obj.ls(attachments_dir, detail=False)
    except FileNotFoundError:
        entries = []

    for entry in entries:
        filename = str(entry).split("/")[-1]
        if filename.startswith(f"{attachment_id}_"):
            fs_obj.rm(entry)
            deleted = True

    if not deleted:
        raise FileNotFoundError(attachment_id)
