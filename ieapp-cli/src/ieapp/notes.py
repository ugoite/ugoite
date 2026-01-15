"""Notes management module."""

from __future__ import annotations

import difflib
import json
import logging
import time
import uuid
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from collections.abc import Mapping
    from pathlib import Path

    import fsspec

import ieapp_core
import yaml

try:  # pragma: no cover - platform specific
    import fcntl

    # declaring a local annotation to make the type checker happy
    fcntl: Any
except ImportError:  # pragma: no cover - platform specific
    # fcntl is not available on Windows/python distributions such as pypy
    fcntl: Any | None = None

from .integrity import IntegrityProvider
from .utils import (
    fs_exists,
    fs_join,
    fs_makedirs,
    fs_read_json,
    fs_write_json,
    get_fs_and_path,
    run_async,
    split_workspace_path,
    storage_config_from_root,
    validate_id,
)

logger = logging.getLogger(__name__)

FRONTMATTER_DELIMITER = "---\n"
H1_PREFIX = "# "
H2_PREFIX = "## "
DEFAULT_INITIAL_MESSAGE = "Initial creation"
DEFAULT_UPDATE_MESSAGE = "Update"
MIN_FRONTMATTER_PARTS = 3


@dataclass
class NotePaths:
    """Paths related to a single note within a workspace."""

    base: str
    note_dir: str
    content: str
    meta: str
    history_dir: str


def _workspace_context(
    workspace_path: str | Path,
    fs: fsspec.AbstractFileSystem | None = None,
) -> tuple[fsspec.AbstractFileSystem, str, str]:
    """Return filesystem, workspace path, and validated workspace id."""
    fs_obj, ws_path = get_fs_and_path(workspace_path, fs)
    workspace_id = ws_path.rstrip("/").split("/")[-1]
    workspace_id = validate_id(workspace_id, "workspace_id")

    if not fs_exists(fs_obj, ws_path):
        msg = f"Workspace {workspace_id} not found"
        raise FileNotFoundError(msg)

    return fs_obj, ws_path, workspace_id


def _note_paths(workspace_path: str, note_id: str) -> NotePaths:
    base = fs_join(workspace_path, "notes")
    note_dir = fs_join(base, note_id) if note_id else base
    return NotePaths(
        base=base,
        note_dir=note_dir,
        content=fs_join(note_dir, "content.json"),
        meta=fs_join(note_dir, "meta.json"),
        history_dir=fs_join(note_dir, "history"),
    )


class NoteExistsError(Exception):
    """Raised when attempting to create a note that already exists."""


class RevisionMismatchError(Exception):
    """Raised when the supplied parent revision does not match the head."""


def _mkdir_secure(fs: fsspec.AbstractFileSystem, path: str, mode: int = 0o700) -> None:
    """Create directory via fsspec with restrictive permissions."""
    fs_makedirs(fs, path, mode=mode, exist_ok=False)


def _extract_title_from_markdown(content: str, fallback: str) -> str:
    """Return the first H1 heading or ``fallback`` if none is present.

    Args:
        content: Raw markdown being parsed.
        fallback: Title to return if no heading is found.

    Returns:
        The extracted title string.

    """
    for line in content.splitlines():
        if line.startswith(H1_PREFIX):
            return line[len(H1_PREFIX) :].strip()
    return fallback


def _parse_markdown(content: str) -> dict[str, Any]:
    """Parse markdown content to extract frontmatter and sections.

    If frontmatter is missing or malformed, it defaults to an empty dictionary.
    If YAML parsing fails, a warning is logged and an empty dictionary is used.

    Args:
        content: Raw markdown string from the editor.

    Returns:
        Dictionary containing ``frontmatter`` and ``sections`` entries.

    """
    frontmatter = {}
    sections = {}

    remaining_content = content

    # Extract Frontmatter
    if content.startswith(FRONTMATTER_DELIMITER):
        try:
            parts = content.split(FRONTMATTER_DELIMITER, 2)
            if len(parts) >= MIN_FRONTMATTER_PARTS:
                fm_str = parts[1]
                frontmatter = yaml.safe_load(fm_str) or {}
                remaining_content = parts[2]
        except yaml.YAMLError as exc:
            logger.warning("Failed to parse frontmatter: %s", exc)

    # Extract H2 Sections
    lines = remaining_content.splitlines()
    current_section = None
    section_content = []

    for line in lines:
        if line.startswith(H2_PREFIX):
            if current_section:
                sections[current_section] = "\n".join(section_content).strip()
            current_section = line[len(H2_PREFIX) :].strip()
            section_content = []
        elif current_section:
            section_content.append(line)

    if current_section:
        sections[current_section] = "\n".join(section_content).strip()

    return {"frontmatter": frontmatter, "sections": sections}


def create_note(
    workspace_path: str | Path,
    note_id: str,
    content: str,
    integrity_provider: IntegrityProvider | None = None,
    author: str = "user",
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> None:
    """Create a note directory with meta, content, and history files.

    Args:
        workspace_path: Absolute path to the workspace directory.
        note_id: Identifier for the note.
        content: Markdown body to persist.
        integrity_provider: Optional override for checksum/signature calculations.
        author: The author of the note (default: "user").
        fs: Optional fsspec filesystem to use.

    Raises:
        NoteExistsError: If the note directory already exists.

    """
    safe_note_id = validate_id(note_id, "note_id")
    if fs is not None or integrity_provider is not None:
        fs_obj, ws_path, workspace_id = _workspace_context(workspace_path, fs)
        paths = _note_paths(ws_path, safe_note_id)

        if fs_exists(fs_obj, paths.note_dir):
            msg = f"Note already exists: {safe_note_id}"
            raise NoteExistsError(msg)

        fs_makedirs(fs_obj, paths.note_dir, mode=0o700, exist_ok=False)
        fs_makedirs(fs_obj, paths.history_dir, mode=0o700, exist_ok=False)

        parsed = _parse_markdown(content)
        frontmatter = parsed.get("frontmatter") or {}
        sections = parsed.get("sections") or {}

        integrity = integrity_provider or IntegrityProvider.for_workspace(
            ws_path,
            fs=fs_obj,
        )
        timestamp = time.time()
        revision_id = uuid.uuid4().hex
        checksum = integrity.checksum(content)
        signature = integrity.signature(content)

        content_payload = {
            "revision_id": revision_id,
            "parent_revision_id": None,
            "author": author,
            "markdown": content,
            "frontmatter": frontmatter,
            "sections": sections,
            "attachments": [],
            "computed": {},
        }
        fs_write_json(fs_obj, paths.content, content_payload)

        history_record = {
            "revision_id": revision_id,
            "parent_revision_id": None,
            "timestamp": timestamp,
            "author": author,
            "diff": "",
            "integrity": {"checksum": checksum, "signature": signature},
            "message": DEFAULT_INITIAL_MESSAGE,
        }
        fs_write_json(
            fs_obj,
            fs_join(paths.history_dir, f"{revision_id}.json"),
            history_record,
        )
        history_index = {
            "note_id": safe_note_id,
            "revisions": [
                {
                    "revision_id": revision_id,
                    "timestamp": timestamp,
                    "checksum": checksum,
                    "signature": signature,
                },
            ],
        }
        fs_write_json(
            fs_obj,
            fs_join(paths.history_dir, "index.json"),
            history_index,
        )

        meta_payload = {
            "id": safe_note_id,
            "workspace_id": workspace_id,
            "title": _extract_title_from_markdown(content, safe_note_id),
            "class": frontmatter.get("class"),
            "tags": frontmatter.get("tags") or [],
            "links": [],
            "canvas_position": {},
            "created_at": timestamp,
            "updated_at": timestamp,
            "integrity": {"checksum": checksum, "signature": signature},
            "deleted": False,
            "deleted_at": None,
            "properties": {},
        }
        fs_write_json(fs_obj, paths.meta, meta_payload)
        return

    root_path, workspace_id = split_workspace_path(workspace_path)
    config = storage_config_from_root(root_path, fs)

    try:
        run_async(
            ieapp_core.create_note,
            config,
            workspace_id,
            safe_note_id,
            content,
            author=author,
        )
    except RuntimeError as exc:
        msg = str(exc)
        if "already exists" in msg:
            raise NoteExistsError(msg) from exc
        if "not found" in msg:
            raise FileNotFoundError(msg) from exc
        raise


def update_note(
    workspace_path: str | Path,
    note_id: str,
    content: str,
    parent_revision_id: str,
    attachments: list[Mapping[str, object]] | None = None,
    integrity_provider: IntegrityProvider | None = None,
    author: str = "user",
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> None:
    """Append a new revision to an existing note.

    Args:
        workspace_path: Absolute path to the workspace directory.
        note_id: Identifier for the note to update.
        content: Updated markdown body.
        parent_revision_id: Revision expected by the caller.
        attachments: Optional list of attachment metadata to persist.
        integrity_provider: Optional override for checksum/signature calculations.
        author: The author of the update (default: "user").
        fs: Optional fsspec filesystem to use.

    Raises:
        FileNotFoundError: If the note directory is missing.
        RevisionMismatchError: If ``parent_revision_id`` does not match head.

    """
    safe_note_id = validate_id(note_id, "note_id")
    if fs is not None or integrity_provider is not None:
        fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path, fs)
        paths = _note_paths(ws_path, safe_note_id)
        if not fs_exists(fs_obj, paths.content):
            msg = f"Note not found: {safe_note_id}"
            raise FileNotFoundError(msg)

        content_json = fs_read_json(fs_obj, paths.content)
        current_revision = content_json.get("revision_id")
        if current_revision != parent_revision_id:
            msg = "Revision conflict"
            raise RevisionMismatchError(msg)

        parsed = _parse_markdown(content)
        frontmatter = parsed.get("frontmatter") or {}
        sections = parsed.get("sections") or {}

        integrity = integrity_provider or IntegrityProvider.for_workspace(
            ws_path,
            fs=fs_obj,
        )
        timestamp = time.time()
        revision_id = uuid.uuid4().hex
        checksum = integrity.checksum(content)
        signature = integrity.signature(content)

        previous_content = content_json.get("markdown", "")
        diff_text = "\n".join(
            difflib.unified_diff(
                str(previous_content).splitlines(),
                content.splitlines(),
                lineterm="",
            ),
        )

        content_json.update(
            {
                "revision_id": revision_id,
                "parent_revision_id": parent_revision_id,
                "author": author,
                "markdown": content,
                "frontmatter": frontmatter,
                "sections": sections,
            },
        )
        if attachments is not None:
            content_json["attachments"] = attachments
        content_json.setdefault("attachments", [])
        content_json.setdefault("computed", {})
        fs_write_json(fs_obj, paths.content, content_json)

        _finalize_revision(
            fs_obj,
            paths.note_dir,
            revision_id,
            parent_revision_id,
            timestamp,
            author,
            diff_text,
            checksum,
            signature,
            content,
            parsed,
        )
        return

    root_path, workspace_id = split_workspace_path(workspace_path)
    config = storage_config_from_root(root_path, fs)

    attachments_json = json.dumps(attachments) if attachments is not None else None
    try:
        run_async(
            ieapp_core.update_note,
            config,
            workspace_id,
            safe_note_id,
            content,
            parent_revision_id=parent_revision_id,
            author=author,
            attachments_json=attachments_json,
        )
    except RuntimeError as exc:
        msg = str(exc)
        if "Revision conflict" in msg:
            raise RevisionMismatchError(msg) from exc
        if "not found" in msg:
            raise FileNotFoundError(msg) from exc
        raise


def _finalize_revision(
    fs: fsspec.AbstractFileSystem,
    note_dir: str,
    rev_id: str,
    parent_revision_id: str,
    timestamp: float,
    author: str,
    diff_text: str,
    checksum: str,
    signature: str,
    content: str,
    parsed: dict[str, Any],
) -> None:
    """Write revision artifacts and update meta/history via fsspec."""
    revision = {
        "revision_id": rev_id,
        "parent_revision_id": parent_revision_id,
        "timestamp": timestamp,
        "author": author,
        "diff": diff_text,
        "integrity": {"checksum": checksum, "signature": signature},
        "message": DEFAULT_UPDATE_MESSAGE,
    }

    history_dir = fs_join(note_dir, "history")
    fs_write_json(fs, fs_join(history_dir, f"{rev_id}.json"), revision)

    index_path = fs_join(history_dir, "index.json")
    history_index = fs_read_json(fs, index_path)
    history_index["revisions"].append(
        {
            "revision_id": rev_id,
            "timestamp": timestamp,
            "checksum": checksum,
            "signature": signature,
        },
    )
    fs_write_json(fs, index_path, history_index)

    meta_path = fs_join(note_dir, "meta.json")
    meta = fs_read_json(fs, meta_path)

    meta["title"] = _extract_title_from_markdown(content, meta.get("title", ""))
    meta["updated_at"] = timestamp
    meta["class"] = parsed["frontmatter"].get("class", meta.get("class"))
    meta["tags"] = parsed["frontmatter"].get("tags", meta.get("tags", []))
    meta["integrity"] = {"checksum": checksum, "signature": signature}

    fs_write_json(fs, meta_path, meta)


def get_note(
    workspace_path: str | Path,
    note_id: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Retrieve a note's content and metadata.

    Args:
        workspace_path: Absolute path to the workspace directory.
        note_id: Identifier for the note.
        fs: Optional fsspec filesystem to use.

    Returns:
        Dictionary containing note content and metadata.

    Raises:
        FileNotFoundError: If the note does not exist or is deleted.

    """
    safe_note_id = validate_id(note_id, "note_id")
    if fs is not None:
        fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path, fs)
        paths = _note_paths(ws_path, safe_note_id)
        if not fs_exists(fs_obj, paths.meta) or not fs_exists(fs_obj, paths.content):
            msg = f"Note not found: {safe_note_id}"
            raise FileNotFoundError(msg)
        meta = fs_read_json(fs_obj, paths.meta)
        if meta.get("deleted") is True:
            msg = f"Note not found: {safe_note_id}"
            raise FileNotFoundError(msg)
        content = fs_read_json(fs_obj, paths.content)
        return {
            "id": safe_note_id,
            "revision_id": content.get("revision_id"),
            "content": content.get("markdown"),
            "frontmatter": content.get("frontmatter") or {},
            "sections": content.get("sections") or {},
            "attachments": content.get("attachments") or [],
            "computed": content.get("computed") or {},
            "title": meta.get("title"),
            "class": meta.get("class"),
            "tags": meta.get("tags") or [],
            "links": meta.get("links") or [],
            "canvas_position": meta.get("canvas_position") or {},
            "created_at": meta.get("created_at"),
            "updated_at": meta.get("updated_at"),
            "integrity": meta.get("integrity") or {},
        }

    root_path, workspace_id = split_workspace_path(workspace_path)
    config = storage_config_from_root(root_path, fs)
    try:
        return run_async(ieapp_core.get_note, config, workspace_id, safe_note_id)
    except RuntimeError as exc:
        msg = str(exc)
        if "not found" in msg:
            raise FileNotFoundError(msg) from exc
        raise


def list_notes(
    workspace_path: str | Path,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> list[dict[str, Any]]:
    """List all notes in a workspace.

    Args:
        workspace_path: Absolute path to the workspace directory.
        fs: Optional fsspec filesystem instance.

    Returns:
        List of note summaries (id, title, class, tags, etc.).

    """
    if fs is not None:
        fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path, fs)
        notes_dir = fs_join(ws_path, "notes")
        if not fs_exists(fs_obj, notes_dir):
            return []
        notes: list[dict[str, Any]] = []
        for note_dir in fs_obj.ls(notes_dir, detail=False):
            meta_path = fs_join(note_dir, "meta.json")
            if not fs_exists(fs_obj, meta_path):
                continue
            meta = fs_read_json(fs_obj, meta_path)
            if meta.get("deleted") is True:
                continue
            notes.append(
                {
                    "id": meta.get("id"),
                    "title": meta.get("title"),
                    "class": meta.get("class"),
                    "tags": meta.get("tags") or [],
                    "properties": meta.get("properties") or {},
                    "links": meta.get("links") or [],
                    "canvas_position": meta.get("canvas_position") or {},
                    "created_at": meta.get("created_at"),
                    "updated_at": meta.get("updated_at"),
                },
            )
        return notes

    root_path, workspace_id = split_workspace_path(workspace_path)
    config = storage_config_from_root(root_path, fs)
    return run_async(ieapp_core.list_notes, config, workspace_id)


def delete_note(
    workspace_path: str | Path,
    note_id: str,
    *,
    hard_delete: bool = False,
    fs: fsspec.AbstractFileSystem | None = None,
) -> None:
    """Tombstone (soft delete) or permanently delete a note.

    Args:
        workspace_path: Absolute path to the workspace directory.
        note_id: Identifier for the note.
        hard_delete: If True, permanently delete. If False, tombstone.
        fs: Optional fsspec filesystem to use.

    Raises:
        FileNotFoundError: If the note does not exist.

    """
    validate_id(note_id, "note_id")
    if fs is not None:
        fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path, fs)
        paths = _note_paths(ws_path, note_id)
        if not fs_exists(fs_obj, paths.note_dir):
            msg = f"Note not found: {note_id}"
            raise FileNotFoundError(msg)
        if hard_delete:
            fs_obj.rm(paths.note_dir, recursive=True)
            return
        meta = fs_read_json(fs_obj, paths.meta)
        meta["deleted"] = True
        meta["deleted_at"] = time.time()
        fs_write_json(fs_obj, paths.meta, meta)
        return

    root_path, workspace_id = split_workspace_path(workspace_path)
    config = storage_config_from_root(root_path, fs)
    try:
        run_async(
            ieapp_core.delete_note,
            config,
            workspace_id,
            note_id,
            hard_delete=hard_delete,
        )
    except RuntimeError as exc:
        msg = str(exc)
        if "not found" in msg:
            raise FileNotFoundError(msg) from exc
        raise


def get_note_history(
    workspace_path: str | Path,
    note_id: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Get the revision history for a note.

    Args:
        workspace_path: Absolute path to the workspace directory.
        note_id: Identifier for the note.
        fs: Optional filesystem for non-local storage.

    Returns:
        Dictionary containing note_id and list of revisions.

    Raises:
        FileNotFoundError: If the note does not exist.

    """
    safe_note_id = validate_id(note_id, "note_id")
    if fs is not None:
        fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path, fs)
        paths = _note_paths(ws_path, safe_note_id)
        history_path = fs_join(paths.history_dir, "index.json")
        if not fs_exists(fs_obj, history_path):
            msg = f"Note not found: {safe_note_id}"
            raise FileNotFoundError(msg)
        return fs_read_json(fs_obj, history_path)

    root_path, workspace_id = split_workspace_path(workspace_path)
    config = storage_config_from_root(root_path)
    try:
        return run_async(
            ieapp_core.get_note_history,
            config,
            workspace_id,
            safe_note_id,
        )
    except RuntimeError as exc:
        msg = str(exc)
        if "not found" in msg:
            raise FileNotFoundError(msg) from exc
        raise


def get_note_revision(
    workspace_path: str | Path,
    note_id: str,
    revision_id: str,
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Get a specific revision of a note.

    Args:
        workspace_path: Absolute path to the workspace directory.
        note_id: Identifier for the note.
        revision_id: The revision ID to retrieve.
        fs: Optional filesystem for non-local storage.

    Returns:
        Dictionary containing the revision data.

    Raises:
        FileNotFoundError: If the note or revision does not exist.

    """
    safe_note_id = validate_id(note_id, "note_id")
    safe_revision_id = validate_id(revision_id, "revision_id")
    if fs is not None:
        fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path, fs)
        paths = _note_paths(ws_path, safe_note_id)
        revision_path = fs_join(paths.history_dir, f"{safe_revision_id}.json")
        if not fs_exists(fs_obj, revision_path):
            msg = f"Note not found: {safe_note_id}"
            raise FileNotFoundError(msg)
        return fs_read_json(fs_obj, revision_path)

    root_path, workspace_id = split_workspace_path(workspace_path)
    config = storage_config_from_root(root_path)
    try:
        return run_async(
            ieapp_core.get_note_revision,
            config,
            workspace_id,
            safe_note_id,
            safe_revision_id,
        )
    except RuntimeError as exc:
        msg = str(exc)
        if "not found" in msg:
            raise FileNotFoundError(msg) from exc
        raise


def restore_note(
    workspace_path: str | Path,
    note_id: str,
    revision_id: str,
    integrity_provider: IntegrityProvider | None = None,
    author: str = "user",
    *,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Restore a note to a previous revision.

    Creates a new revision that records the intent to restore to the specified
    revision. Note that this implementation creates a "restore marker" revision
    only; the actual note content is NOT replaced with the target revision's
    content. Full time travel requires storing the complete markdown in each
    revision file (planned for a future milestone).

    Args:
        workspace_path: Absolute path to the workspace directory.
        note_id: Identifier for the note.
        revision_id: The revision ID to restore to.
        integrity_provider: Optional override for checksum/signature calculations.
        author: The author of the restore operation.
        fs: Optional fsspec filesystem to use.

    Returns:
        Dictionary containing the new revision info.

    Raises:
        FileNotFoundError: If the note or revision does not exist.

    """
    _ = integrity_provider
    safe_note_id = validate_id(note_id, "note_id")
    safe_revision_id = validate_id(revision_id, "revision_id")
    root_path, workspace_id = split_workspace_path(workspace_path)
    config = storage_config_from_root(root_path, fs)
    try:
        return run_async(
            ieapp_core.restore_note,
            config,
            workspace_id,
            safe_note_id,
            safe_revision_id,
            author=author,
        )
    except RuntimeError as exc:
        msg = str(exc)
        if "not found" in msg:
            raise FileNotFoundError(msg) from exc
        raise
