"""Notes management module."""

import difflib
import json
import logging
import shutil
import time
import uuid
from pathlib import Path
from typing import Any

import yaml

try:  # pragma: no cover - platform specific
    import fcntl

    # declaring a local annotation to make the type checker happy
    fcntl: Any
except ImportError:  # pragma: no cover - platform specific
    # fcntl is not available on Windows/python distributions such as pypy
    fcntl: Any | None = None

from .integrity import IntegrityProvider
from .utils import safe_resolve_path, validate_id, write_json_secure

logger = logging.getLogger(__name__)

FRONTMATTER_DELIMITER = "---\n"
H1_PREFIX = "# "
H2_PREFIX = "## "
DEFAULT_INITIAL_MESSAGE = "Initial creation"
DEFAULT_UPDATE_MESSAGE = "Update"
MIN_FRONTMATTER_PARTS = 3


class NoteExistsError(Exception):
    """Raised when attempting to create a note that already exists."""


class RevisionMismatchError(Exception):
    """Raised when the supplied parent revision does not match the head."""


def _mkdir_secure(path: Path, mode: int = 0o700) -> None:
    """Create ``path`` with restrictive permissions from the outset.

    Args:
        path: Directory to create.
        mode: Permission bits applied during creation.

    """
    path.mkdir(mode=mode)


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
) -> None:
    """Create a note directory with meta, content, and history files.

    Args:
        workspace_path: Absolute path to the workspace directory.
        note_id: Identifier for the note.
        content: Markdown body to persist.
        integrity_provider: Optional override for checksum/signature calculations.
        author: The author of the note (default: "user").

    Raises:
        NoteExistsError: If the note directory already exists.

    """
    # Validate and sanitize note_id
    safe_note_id = validate_id(note_id, "note_id")

    ws_path = Path(workspace_path).resolve()
    # Use safe path resolution to prevent path traversal
    note_dir = safe_resolve_path(ws_path, "notes", safe_note_id)

    if note_dir.exists():
        msg = f"Note {safe_note_id} already exists"
        raise NoteExistsError(msg)

    _mkdir_secure(note_dir)

    # Parse content
    parsed = _parse_markdown(content)
    provider = integrity_provider or IntegrityProvider.for_workspace(ws_path)

    # Create initial revision
    rev_id = str(uuid.uuid4())
    timestamp = time.time()
    checksum = provider.checksum(content)
    signature = provider.signature(content)

    # Create content.json
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

    content_path = note_dir / "content.json"
    write_json_secure(content_path, content_data, exclusive=True)

    revision = {
        "revision_id": rev_id,
        "parent_revision_id": None,
        "timestamp": timestamp,
        "author": author,
        "diff": "",
        "integrity": {"checksum": checksum, "signature": signature},
        "message": DEFAULT_INITIAL_MESSAGE,
    }

    history_dir = note_dir / "history"
    _mkdir_secure(history_dir)

    rev_path = history_dir / f"{rev_id}.json"
    write_json_secure(rev_path, revision, exclusive=True)

    # Update history index
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
    index_path = history_dir / "index.json"
    write_json_secure(index_path, history_index, exclusive=True)

    # Create meta.json
    # Extract title from first H1 or use note_id
    title = _extract_title_from_markdown(content, note_id)

    # Read workspace_id from workspace meta.json if available
    workspace_meta_file = ws_path / "meta.json"
    if workspace_meta_file.exists():
        with workspace_meta_file.open("r") as f:
            workspace_meta = json.load(f)
            workspace_id = workspace_meta.get("id", ws_path.name)
    else:
        workspace_id = ws_path.name

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

    meta_path = note_dir / "meta.json"
    write_json_secure(meta_path, meta, exclusive=True)


def update_note(
    workspace_path: str | Path,
    note_id: str,
    content: str,
    parent_revision_id: str,
    integrity_provider: IntegrityProvider | None = None,
    author: str = "user",
) -> None:
    """Append a new revision to an existing note.

    Args:
        workspace_path: Absolute path to the workspace directory.
        note_id: Identifier for the note to update.
        content: Updated markdown body.
        parent_revision_id: Revision expected by the caller.
        integrity_provider: Optional override for checksum/signature calculations.
        author: The author of the update (default: "user").

    Raises:
        FileNotFoundError: If the note directory is missing.
        RevisionMismatchError: If ``parent_revision_id`` does not match head.

    """
    # Validate and sanitize note_id
    safe_note_id = validate_id(note_id, "note_id")

    ws_path = Path(workspace_path).resolve()
    # Use safe path resolution to prevent path traversal
    note_dir = safe_resolve_path(ws_path, "notes", safe_note_id)

    if not note_dir.exists():
        msg = f"Note {safe_note_id} not found"
        raise FileNotFoundError(msg)

    # Check parent revision from content.json (as per spec)
    content_path = note_dir / "content.json"

    # Use advisory locking to prevent race conditions
    # We open in r+ mode to read and then write
    with content_path.open("r+", encoding="utf-8") as f:
        if fcntl:
            fcntl.flock(f, fcntl.LOCK_EX)

        try:
            current_content_data = json.load(f)

            if current_content_data["revision_id"] != parent_revision_id:
                msg = (
                    "Revision conflict: the note has been modified. "
                    f"Current revision: {current_content_data['revision_id']}, "
                    f"provided revision: {parent_revision_id}"
                )
                raise RevisionMismatchError(msg)

            # Parse content
            parsed = _parse_markdown(content)
            provider = integrity_provider or IntegrityProvider.for_workspace(ws_path)

            # Create new revision
            rev_id = str(uuid.uuid4())
            timestamp = time.time()
            checksum = provider.checksum(content)
            signature = provider.signature(content)

            # Calculate diff
            diff = difflib.unified_diff(
                current_content_data["markdown"].splitlines(keepends=True),
                content.splitlines(keepends=True),
                fromfile=f"revision/{current_content_data['revision_id']}",
                tofile=f"revision/{rev_id}",
            )
            diff_text = "".join(diff)

            # Update content.json
            content_data = {
                "revision_id": rev_id,
                "parent_revision_id": parent_revision_id,
                "author": author,
                "markdown": content,
                "frontmatter": parsed["frontmatter"],
                "sections": parsed["sections"],
                "attachments": current_content_data.get("attachments", []),
                "computed": current_content_data.get("computed", {}),
            }

            # Write back to content.json
            f.seek(0)
            json.dump(content_data, f, indent=2)
            f.truncate()

        finally:
            if fcntl:
                fcntl.flock(f, fcntl.LOCK_UN)

    revision = {
        "revision_id": rev_id,
        "parent_revision_id": parent_revision_id,
        "timestamp": timestamp,
        "author": author,
        "diff": diff_text,
        "integrity": {"checksum": checksum, "signature": signature},
        "message": DEFAULT_UPDATE_MESSAGE,
    }

    history_dir = note_dir / "history"
    rev_path = history_dir / f"{rev_id}.json"
    write_json_secure(rev_path, revision)

    # Update history index
    index_path = history_dir / "index.json"
    with index_path.open("r", encoding="utf-8") as f:
        history_index = json.load(f)

    history_index["revisions"].append(
        {
            "revision_id": rev_id,
            "timestamp": timestamp,
            "checksum": checksum,
            "signature": signature,
        },
    )

    write_json_secure(index_path, history_index)

    # Update meta.json
    meta_path = note_dir / "meta.json"
    with meta_path.open("r", encoding="utf-8") as f:
        meta = json.load(f)

    # Update title if changed
    meta["title"] = _extract_title_from_markdown(content, meta["title"])
    meta["updated_at"] = timestamp
    meta["class"] = parsed["frontmatter"].get("class", meta.get("class"))
    meta["tags"] = parsed["frontmatter"].get("tags", meta.get("tags", []))
    meta["integrity"] = {"checksum": checksum, "signature": signature}

    write_json_secure(meta_path, meta)


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
    # Validate and sanitize note_id
    safe_note_id = validate_id(note_id, "note_id")

    ws_path = Path(workspace_path).resolve()
    # Use safe path resolution to prevent path traversal
    note_dir = safe_resolve_path(ws_path, "notes", safe_note_id)

    if not note_dir.exists():
        msg = f"Note {safe_note_id} not found"
        raise FileNotFoundError(msg)

    content_path = note_dir / "content.json"
    meta_path = note_dir / "meta.json"

    with content_path.open("r", encoding="utf-8") as f:
        content_data = json.load(f)

    with meta_path.open("r", encoding="utf-8") as f:
        meta = json.load(f)

    # Check if note is soft-deleted (tombstoned)
    if meta.get("deleted"):
        msg = f"Note {note_id} not found"
        raise FileNotFoundError(msg)

    return {
        "id": note_id,
        "revision_id": content_data.get("revision_id"),
        "content": content_data.get("markdown"),  # API uses "content" for frontend
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


def list_notes(workspace_path: str | Path) -> list[dict[str, Any]]:
    """List all notes in a workspace.

    Args:
        workspace_path: Absolute path to the workspace directory.

    Returns:
        List of note summaries (id, title, class, tags, etc.).

    """
    ws_path = Path(workspace_path)
    notes_dir = ws_path / "notes"

    if not notes_dir.exists():
        return []

    notes = []
    for note_dir in notes_dir.iterdir():
        if note_dir.is_dir():
            meta_path = note_dir / "meta.json"
            if meta_path.exists():
                try:
                    with meta_path.open("r", encoding="utf-8") as f:
                        meta = json.load(f)
                except (json.JSONDecodeError, OSError) as e:
                    logger.warning(
                        "Could not read metadata for note at %s: %s",
                        meta_path,
                        e,
                    )
                    continue
                # Skip tombstoned notes
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
) -> None:
    """Tombstone (soft delete) or permanently delete a note.

    Args:
        workspace_path: Absolute path to the workspace directory.
        note_id: Identifier for the note.
        hard_delete: If True, permanently delete. If False, tombstone.

    Raises:
        FileNotFoundError: If the note does not exist.

    """
    validate_id(note_id, "note_id")

    ws_path = Path(workspace_path).resolve()
    # Use safe path resolution to prevent path traversal
    note_dir = safe_resolve_path(ws_path, "notes", note_id)

    if not note_dir.exists():
        msg = f"Note {note_id} not found"
        raise FileNotFoundError(msg)

    if hard_delete:
        shutil.rmtree(note_dir)
    else:
        # Soft delete - mark as deleted in meta.json
        meta_path = note_dir / "meta.json"
        with meta_path.open("r", encoding="utf-8") as f:
            meta = json.load(f)

        meta["deleted"] = True
        meta["deleted_at"] = time.time()

        write_json_secure(meta_path, meta)


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
    # Validate and sanitize note_id
    safe_note_id = validate_id(note_id, "note_id")

    ws_path = Path(workspace_path).resolve()
    # Use safe path resolution to prevent path traversal
    note_dir = safe_resolve_path(ws_path, "notes", safe_note_id)

    if not note_dir.exists():
        msg = f"Note {safe_note_id} not found"
        raise FileNotFoundError(msg)

    history_index_path = note_dir / "history" / "index.json"

    if not history_index_path.exists():
        return {"note_id": safe_note_id, "revisions": []}

    with history_index_path.open("r", encoding="utf-8") as f:
        return json.load(f)


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
    # Validate and sanitize IDs
    safe_note_id = validate_id(note_id, "note_id")
    safe_revision_id = validate_id(revision_id, "revision_id")

    ws_path = Path(workspace_path).resolve()
    # Use safe path resolution to prevent path traversal
    note_dir = safe_resolve_path(ws_path, "notes", safe_note_id)

    if not note_dir.exists():
        msg = f"Note {safe_note_id} not found"
        raise FileNotFoundError(msg)

    # revision_id is validated above, safe to use in path
    revision_path = note_dir / "history" / f"{safe_revision_id}.json"

    if not revision_path.exists():
        msg = f"Revision {safe_revision_id} not found for note {safe_note_id}"
        raise FileNotFoundError(msg)

    with revision_path.open("r", encoding="utf-8") as f:
        return json.load(f)


def restore_note(
    workspace_path: str | Path,
    note_id: str,
    revision_id: str,
    integrity_provider: IntegrityProvider | None = None,
    author: str = "user",
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

    Returns:
        Dictionary containing the new revision info.

    Raises:
        FileNotFoundError: If the note or revision does not exist.

    """
    # Validate and sanitize IDs
    safe_note_id = validate_id(note_id, "note_id")
    safe_revision_id = validate_id(revision_id, "revision_id")

    ws_path = Path(workspace_path).resolve()
    # Use safe path resolution to prevent path traversal
    note_dir = safe_resolve_path(ws_path, "notes", safe_note_id)

    if not note_dir.exists():
        msg = f"Note {safe_note_id} not found"
        raise FileNotFoundError(msg)

    # Verify target revision exists (revision_id is validated above)
    target_revision_path = note_dir / "history" / f"{safe_revision_id}.json"
    if not target_revision_path.exists():
        msg = f"Revision {safe_revision_id} not found for note {safe_note_id}"
        raise FileNotFoundError(msg)

    # Read current content to get parent_revision_id for the new revision
    content_path = note_dir / "content.json"
    with content_path.open("r", encoding="utf-8") as f:
        current_content_data = json.load(f)

    current_revision_id = current_content_data["revision_id"]

    # Verify revision is in history index
    history_index_path = note_dir / "history" / "index.json"
    with history_index_path.open("r", encoding="utf-8") as f:
        history_index = json.load(f)

    revision_order = [r["revision_id"] for r in history_index["revisions"]]
    if revision_id not in revision_order:
        msg = f"Revision {revision_id} not found in history index"
        raise FileNotFoundError(msg)

    # Note: Full time travel (restoring arbitrary revision content) requires
    # storing full markdown in revision files. Current implementation creates
    # a "restore marker" revision referencing the chosen ancestor.
    provider = integrity_provider or IntegrityProvider.for_workspace(ws_path)

    new_rev_id = str(uuid.uuid4())
    timestamp = time.time()

    # Keep current content and mark the restore operation in revision metadata
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

    # Write the revision
    history_dir = note_dir / "history"
    rev_path = history_dir / f"{new_rev_id}.json"
    write_json_secure(rev_path, revision)

    # Update history index
    history_index["revisions"].append(
        {
            "revision_id": new_rev_id,
            "timestamp": timestamp,
            "checksum": checksum,
            "signature": signature,
        },
    )
    write_json_secure(history_index_path, history_index)

    # Update content.json with the new revision_id
    content_data = current_content_data.copy()
    content_data["revision_id"] = new_rev_id
    content_data["parent_revision_id"] = current_revision_id

    write_json_secure(content_path, content_data)

    # Update meta.json
    meta_path = note_dir / "meta.json"
    with meta_path.open("r", encoding="utf-8") as f:
        meta = json.load(f)

    meta["updated_at"] = timestamp
    meta["integrity"] = {"checksum": checksum, "signature": signature}

    write_json_secure(meta_path, meta)

    return {
        "revision_id": new_rev_id,
        "restored_from": revision_id,
        "timestamp": timestamp,
    }
