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
    from pathlib import Path

    import fsspec

import yaml

try:  # pragma: no cover - platform specific
    import fcntl

    # declaring a local annotation to make the type checker happy
    fcntl: Any
except ImportError:  # pragma: no cover - platform specific
    # fcntl is not available on Windows/python distributions such as pypy
    fcntl: Any | None = None

from .indexer import Indexer
from .integrity import IntegrityProvider
from .utils import (
    fs_exists,
    fs_join,
    fs_makedirs,
    fs_read_json,
    fs_write_json,
    get_fs_and_path,
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

    fs_obj, ws_path, workspace_id = _workspace_context(workspace_path, fs)

    notes_dir = fs_join(ws_path, "notes")
    if not fs_exists(fs_obj, notes_dir):
        msg = f"Notes directory missing for workspace {workspace_id}"
        raise FileNotFoundError(msg)

    note_dir = fs_join(notes_dir, safe_note_id)
    if fs_exists(fs_obj, note_dir):
        msg = f"Note {safe_note_id} already exists"
        raise NoteExistsError(msg)

    _mkdir_secure(fs_obj, note_dir)

    parsed = _parse_markdown(content)
    provider = integrity_provider or IntegrityProvider.for_workspace(ws_path, fs=fs_obj)

    rev_id = str(uuid.uuid4())
    timestamp = time.time()
    checksum = provider.checksum(content)
    signature = provider.signature(content)

    content_data = {
        "revision_id": rev_id,
        "parent_revision_id": None,
        "author": author,
        "markdown": content,
        "frontmatter": parsed["frontmatter"],
        "sections": parsed["sections"],
        "attachments": [],
        "computed": {},
    }
    fs_write_json(
        fs_obj,
        fs_join(note_dir, "content.json"),
        content_data,
        exclusive=True,
    )

    revision = {
        "revision_id": rev_id,
        "parent_revision_id": None,
        "timestamp": timestamp,
        "author": author,
        "diff": "",
        "integrity": {"checksum": checksum, "signature": signature},
        "message": DEFAULT_INITIAL_MESSAGE,
    }

    history_dir = fs_join(note_dir, "history")
    _mkdir_secure(fs_obj, history_dir)
    fs_write_json(
        fs_obj,
        fs_join(history_dir, f"{rev_id}.json"),
        revision,
        exclusive=True,
    )

    history_index = {
        "note_id": note_id,
        "revisions": [
            {
                "revision_id": rev_id,
                "timestamp": timestamp,
                "checksum": checksum,
                "signature": signature,
            },
        ],
    }
    fs_write_json(
        fs_obj,
        fs_join(history_dir, "index.json"),
        history_index,
        exclusive=True,
    )

    title = _extract_title_from_markdown(content, note_id)
    workspace_meta_path = fs_join(ws_path, "meta.json")
    if fs_exists(fs_obj, workspace_meta_path):
        workspace_meta = fs_read_json(fs_obj, workspace_meta_path)
        workspace_id = workspace_meta.get("id", workspace_id)

    meta = {
        "id": note_id,
        "workspace_id": workspace_id,
        "title": title,
        "class": parsed["frontmatter"].get("class"),
        "tags": parsed["frontmatter"].get("tags", []),
        "links": [],
        "canvas_position": {},
        "created_at": timestamp,
        "updated_at": timestamp,
        "integrity": {"checksum": checksum, "signature": signature},
    }

    fs_write_json(fs_obj, fs_join(note_dir, "meta.json"), meta, exclusive=True)

    # Refresh the workspace index
    Indexer(str(ws_path), fs=fs_obj).run_once()


def update_note(
    workspace_path: str | Path,
    note_id: str,
    content: str,
    parent_revision_id: str,
    attachments: list[dict[str, Any]] | None = None,
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
    fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path, fs)

    note_dir = fs_join(ws_path, "notes", safe_note_id)
    if not fs_exists(fs_obj, note_dir):
        msg = f"Note {safe_note_id} not found"
        raise FileNotFoundError(msg)

    content_path = fs_join(note_dir, "content.json")
    if not fs_exists(fs_obj, content_path):
        msg = f"Note content not found for {safe_note_id}"
        raise FileNotFoundError(msg)

    current_content_data = fs_read_json(fs_obj, content_path)

    if current_content_data["revision_id"] != parent_revision_id:
        msg = (
            "Revision conflict: the note has been modified. "
            f"Current revision: {current_content_data['revision_id']}, "
            f"provided revision: {parent_revision_id}"
        )
        raise RevisionMismatchError(msg)

    parsed = _parse_markdown(content)
    provider = integrity_provider or IntegrityProvider.for_workspace(
        ws_path,
        fs=fs_obj,
    )

    rev_id = str(uuid.uuid4())
    timestamp = time.time()
    checksum = provider.checksum(content)
    signature = provider.signature(content)

    diff = difflib.unified_diff(
        current_content_data["markdown"].splitlines(keepends=True),
        content.splitlines(keepends=True),
        fromfile=f"revision/{current_content_data['revision_id']}",
        tofile=f"revision/{rev_id}",
    )
    diff_text = "".join(diff)

    content_data = {
        "revision_id": rev_id,
        "parent_revision_id": parent_revision_id,
        "author": author,
        "markdown": content,
        "frontmatter": parsed["frontmatter"],
        "sections": parsed["sections"],
        "attachments": attachments
        if attachments is not None
        else current_content_data.get("attachments", []),
        "computed": current_content_data.get("computed", {}),
    }

    fs_write_json(fs_obj, content_path, content_data)

    _finalize_revision(
        fs_obj,
        note_dir,
        rev_id,
        parent_revision_id,
        timestamp,
        author,
        diff_text,
        checksum,
        signature,
        content,
        parsed,
    )

    # Refresh the workspace index
    Indexer(str(ws_path), fs=fs_obj).run_once()


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


def get_note(workspace_path: str | Path, note_id: str) -> dict[str, Any]:
    """Retrieve a note's content and metadata.

    Args:
        workspace_path: Absolute path to the workspace directory.
        note_id: Identifier for the note.

    Returns:
        Dictionary containing note content and metadata.

    Raises:
        FileNotFoundError: If the note does not exist or is deleted.

    """
    safe_note_id = validate_id(note_id, "note_id")
    fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path)

    note_dir = fs_join(ws_path, "notes", safe_note_id)
    if not fs_exists(fs_obj, note_dir):
        msg = f"Note {safe_note_id} not found"
        raise FileNotFoundError(msg)

    content_path = fs_join(note_dir, "content.json")
    meta_path = fs_join(note_dir, "meta.json")

    if not fs_exists(fs_obj, content_path) or not fs_exists(fs_obj, meta_path):
        msg = f"Note {safe_note_id} not found"
        raise FileNotFoundError(msg)

    content_data = fs_read_json(fs_obj, content_path)
    meta = fs_read_json(fs_obj, meta_path)

    if meta.get("deleted"):
        msg = f"Note {note_id} not found"
        raise FileNotFoundError(msg)

    return {
        "id": note_id,
        "revision_id": content_data.get("revision_id"),
        "content": content_data.get("markdown"),
        "frontmatter": content_data.get("frontmatter", {}),
        "sections": content_data.get("sections", {}),
        "attachments": content_data.get("attachments", []),
        "computed": content_data.get("computed", {}),
        "title": meta.get("title"),
        "class": meta.get("class"),
        "tags": meta.get("tags", []),
        "links": meta.get("links", []),
        "canvas_position": meta.get("canvas_position", {}),
        "created_at": meta.get("created_at"),
        "updated_at": meta.get("updated_at"),
        "integrity": meta.get("integrity", {}),
    }


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
    fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path, fs=fs)
    notes_dir = fs_join(ws_path, "notes")

    if not fs_exists(fs_obj, notes_dir):
        return []

    try:
        entries = fs_obj.ls(notes_dir, detail=True)
    except FileNotFoundError:
        return []

    notes: list[dict[str, Any]] = []
    for entry in entries:
        if isinstance(entry, dict):
            if entry.get("type") != "directory":
                continue
            note_dir = entry.get("name") or entry.get("path") or ""
        else:
            note_dir = str(entry)

        meta_path = fs_join(note_dir, "meta.json")
        if not fs_exists(fs_obj, meta_path):
            continue

        try:
            meta = fs_read_json(fs_obj, meta_path)
        except (json.JSONDecodeError, OSError) as e:
            logger.warning("Could not read metadata for note at %s: %s", meta_path, e)
            continue

        if meta.get("deleted"):
            continue

        notes.append(
            {
                "id": meta.get("id"),
                "title": meta.get("title"),
                "class": meta.get("class"),
                "tags": meta.get("tags", []),
                "properties": meta.get("properties", {}),
                "links": meta.get("links", []),
                "canvas_position": meta.get("canvas_position", {}),
                "created_at": meta.get("created_at"),
                "updated_at": meta.get("updated_at"),
            },
        )

    return notes


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
    fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path, fs)

    note_dir = fs_join(ws_path, "notes", note_id)
    if not fs_exists(fs_obj, note_dir):
        msg = f"Note {note_id} not found"
        raise FileNotFoundError(msg)

    if hard_delete:
        fs_obj.rm(note_dir, recursive=True)
        # Refresh the workspace index
        Indexer(str(ws_path), fs=fs_obj).run_once()
        return

    meta_path = fs_join(note_dir, "meta.json")
    if not fs_exists(fs_obj, meta_path):
        msg = f"Note {note_id} not found"
        raise FileNotFoundError(msg)

    meta = fs_read_json(fs_obj, meta_path)
    meta["deleted"] = True
    meta["deleted_at"] = time.time()

    fs_write_json(fs_obj, meta_path, meta)

    # Refresh the workspace index
    Indexer(str(ws_path), fs=fs_obj).run_once()


def get_note_history(workspace_path: str | Path, note_id: str) -> dict[str, Any]:
    """Get the revision history for a note.

    Args:
        workspace_path: Absolute path to the workspace directory.
        note_id: Identifier for the note.

    Returns:
        Dictionary containing note_id and list of revisions.

    Raises:
        FileNotFoundError: If the note does not exist.

    """
    safe_note_id = validate_id(note_id, "note_id")
    fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path)

    note_dir = fs_join(ws_path, "notes", safe_note_id)
    if not fs_exists(fs_obj, note_dir):
        msg = f"Note {safe_note_id} not found"
        raise FileNotFoundError(msg)

    history_index_path = fs_join(note_dir, "history", "index.json")
    if not fs_exists(fs_obj, history_index_path):
        return {"note_id": safe_note_id, "revisions": []}

    return fs_read_json(fs_obj, history_index_path)


def get_note_revision(
    workspace_path: str | Path,
    note_id: str,
    revision_id: str,
) -> dict[str, Any]:
    """Get a specific revision of a note.

    Args:
        workspace_path: Absolute path to the workspace directory.
        note_id: Identifier for the note.
        revision_id: The revision ID to retrieve.

    Returns:
        Dictionary containing the revision data.

    Raises:
        FileNotFoundError: If the note or revision does not exist.

    """
    safe_note_id = validate_id(note_id, "note_id")
    safe_revision_id = validate_id(revision_id, "revision_id")

    fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path)
    note_dir = fs_join(ws_path, "notes", safe_note_id)
    if not fs_exists(fs_obj, note_dir):
        msg = f"Note {safe_note_id} not found"
        raise FileNotFoundError(msg)

    revision_path = fs_join(note_dir, "history", f"{safe_revision_id}.json")
    if not fs_exists(fs_obj, revision_path):
        msg = f"Revision {safe_revision_id} not found for note {safe_note_id}"
        raise FileNotFoundError(msg)

    return fs_read_json(fs_obj, revision_path)


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
    safe_note_id = validate_id(note_id, "note_id")
    safe_revision_id = validate_id(revision_id, "revision_id")

    fs_obj, ws_path, _workspace_id = _workspace_context(workspace_path, fs)
    note_dir = fs_join(ws_path, "notes", safe_note_id)
    if not fs_exists(fs_obj, note_dir):
        msg = f"Note {safe_note_id} not found"
        raise FileNotFoundError(msg)

    revision_path = fs_join(note_dir, "history", f"{safe_revision_id}.json")
    if not fs_exists(fs_obj, revision_path):
        msg = f"Revision {safe_revision_id} not found for note {safe_note_id}"
        raise FileNotFoundError(msg)

    content_path = fs_join(note_dir, "content.json")
    if not fs_exists(fs_obj, content_path):
        msg = f"Note {safe_note_id} content missing"
        raise FileNotFoundError(msg)

    current_content_data = fs_read_json(fs_obj, content_path)
    current_revision_id = current_content_data["revision_id"]

    history_index_path = fs_join(note_dir, "history", "index.json")
    history_index = fs_read_json(fs_obj, history_index_path)
    revision_order = [r["revision_id"] for r in history_index.get("revisions", [])]
    if revision_id not in revision_order:
        msg = f"Revision {revision_id} not found in history index"
        raise FileNotFoundError(msg)

    provider = integrity_provider or IntegrityProvider.for_workspace(ws_path, fs=fs_obj)

    new_rev_id = str(uuid.uuid4())
    timestamp = time.time()
    content = current_content_data["markdown"]
    checksum = provider.checksum(content)
    signature = provider.signature(content)

    revision = {
        "revision_id": new_rev_id,
        "parent_revision_id": current_revision_id,
        "timestamp": timestamp,
        "author": author,
        "diff": "",
        "integrity": {"checksum": checksum, "signature": signature},
        "message": f"Restored from revision {revision_id}",
        "restored_from": revision_id,
    }

    history_dir = fs_join(note_dir, "history")
    fs_write_json(fs_obj, fs_join(history_dir, f"{new_rev_id}.json"), revision)

    history_index["revisions"].append(
        {
            "revision_id": new_rev_id,
            "timestamp": timestamp,
            "checksum": checksum,
            "signature": signature,
        },
    )
    fs_write_json(fs_obj, history_index_path, history_index)

    content_data = current_content_data.copy()
    content_data["revision_id"] = new_rev_id
    content_data["parent_revision_id"] = current_revision_id
    fs_write_json(fs_obj, content_path, content_data)

    meta_path = fs_join(note_dir, "meta.json")
    meta = fs_read_json(fs_obj, meta_path)
    meta["updated_at"] = timestamp
    meta["integrity"] = {"checksum": checksum, "signature": signature}
    fs_write_json(fs_obj, meta_path, meta)

    # Refresh the workspace index
    Indexer(str(ws_path), fs=fs_obj).run_once()

    return {
        "revision_id": new_rev_id,
        "restored_from": revision_id,
        "timestamp": timestamp,
    }
