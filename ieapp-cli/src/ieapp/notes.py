import difflib
import json
import logging
import os
import time
import uuid
from pathlib import Path
from typing import Any, Dict, Optional, Union

import yaml

from .integrity import IntegrityProvider

logger = logging.getLogger(__name__)

FRONTMATTER_DELIMITER = "---\n"
H1_PREFIX = "# "
H2_PREFIX = "## "
DEFAULT_INITIAL_MESSAGE = "Initial creation"
DEFAULT_UPDATE_MESSAGE = "Update"


class NoteExistsError(Exception):
    """Raised when attempting to create a note that already exists."""

    pass


class RevisionMismatchError(Exception):
    """Raised when the supplied parent revision does not match the head."""

    pass


def _mkdir_secure(path: Path, mode: int = 0o700) -> None:
    """Creates a directory with restrictive permissions.

    Args:
        path: Directory to create.
        mode: Permission bits applied at creation.
    """

    path.mkdir(mode=mode, parents=False)


def _write_json_secure(path: Path, payload: Dict[str, Any], mode: int = 0o600) -> None:
    """Writes JSON to ``path`` while setting permissions atomically.

    Args:
        path: Target file path.
        payload: JSON-serializable dictionary.
        mode: Permission bits applied at creation.
    """

    fd = os.open(str(path), os.O_WRONLY | os.O_CREAT | os.O_TRUNC, mode)
    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2)


def _extract_title_from_markdown(content: str, fallback: str) -> str:
    """Returns the first H1 heading or ``fallback`` if none is present.

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


def _parse_markdown(content: str) -> Dict[str, Any]:
    """Parses markdown content to extract frontmatter and sections.

    Args:
        content: Raw markdown string from the editor.

    Returns:
        Dictionary containing ``frontmatter`` and ``sections`` entries.
    """
    frontmatter = {}
    sections = {}

    # Extract Frontmatter
    if content.startswith(FRONTMATTER_DELIMITER):
        try:
            parts = content.split(FRONTMATTER_DELIMITER, 2)
            if len(parts) >= 3:
                fm_str = parts[1]
                frontmatter = yaml.safe_load(fm_str) or {}
        except yaml.YAMLError as exc:
            logger.warning("Failed to parse frontmatter: %s", exc)

    # Extract H2 Sections
    lines = content.splitlines()
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
    workspace_path: Union[str, Path],
    note_id: str,
    content: str,
    integrity_provider: Optional[IntegrityProvider] = None,
    author: str = "user",
) -> None:
    """Creates a note directory with meta, content, and history files.

    Args:
        workspace_path: Absolute path to the workspace directory.
        note_id: Identifier for the note.
        content: Markdown body to persist.
        integrity_provider: Optional override for checksum/signature calculations.
        author: The author of the note (default: "user").

    Raises:
        NoteExistsError: If the note directory already exists.
    """
    ws_path = Path(workspace_path)
    note_dir = ws_path / "notes" / note_id

    if note_dir.exists():
        raise NoteExistsError(f"Note {note_id} already exists")

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
    _write_json_secure(content_path, content_data)

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
    _write_json_secure(rev_path, revision)

    # Update history index
    history_index = {
        "note_id": note_id,
        "revisions": [
            {
                "revision_id": rev_id,
                "timestamp": timestamp,
                "checksum": checksum,
                "signature": signature,
            }
        ],
    }
    index_path = history_dir / "index.json"
    _write_json_secure(index_path, history_index)

    # Create meta.json
    # Extract title from first H1 or use note_id
    title = _extract_title_from_markdown(content, note_id)

    meta = {
        "id": note_id,
        "workspace_id": ws_path.name,  # Infer workspace_id
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
    _write_json_secure(meta_path, meta)


def update_note(
    workspace_path: Union[str, Path],
    note_id: str,
    content: str,
    parent_revision_id: str,
    integrity_provider: Optional[IntegrityProvider] = None,
    author: str = "user",
) -> None:
    """Appends a new revision to an existing note.

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
    ws_path = Path(workspace_path)
    note_dir = ws_path / "notes" / note_id

    if not note_dir.exists():
        raise FileNotFoundError(f"Note {note_id} not found")

    # Check parent revision from content.json (as per spec)
    content_path = note_dir / "content.json"
    with content_path.open("r", encoding="utf-8") as f:
        current_content_data = json.load(f)

    if current_content_data["revision_id"] != parent_revision_id:
        raise RevisionMismatchError(
            "Revision conflict: the note has been modified. "
            f"Current revision: {current_content_data['revision_id']}, "
            f"provided revision: {parent_revision_id}"
        )

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

    _write_json_secure(content_path, content_data)

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
    _write_json_secure(rev_path, revision)

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
        }
    )

    _write_json_secure(index_path, history_index)

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

    _write_json_secure(meta_path, meta)
