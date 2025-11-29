import json
import os
import time
import uuid
import logging
import difflib
from pathlib import Path
from typing import Any, Dict, Optional, Union

import fsspec
import yaml

from .integrity import IntegrityProvider

logger = logging.getLogger(__name__)


class NoteExistsError(Exception):
    pass


class RevisionMismatchError(Exception):
    pass


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
    if content.startswith("---\n"):
        try:
            parts = content.split("---\n", 2)
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
        if line.startswith("## "):
            if current_section:
                sections[current_section] = "\n".join(section_content).strip()
            current_section = line[3:].strip()
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
    ws_path_str = str(workspace_path)
    fs = fsspec.filesystem("file")

    note_dir = os.path.join(ws_path_str, "notes", note_id)

    if fs.exists(note_dir):
        raise NoteExistsError(f"Note {note_id} already exists")

    fs.makedirs(note_dir)
    os.chmod(note_dir, 0o700)

    # Parse content
    parsed = _parse_markdown(content)
    provider = integrity_provider or IntegrityProvider.for_workspace(workspace_path)

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

    content_path = os.path.join(note_dir, "content.json")
    with fs.open(content_path, "w") as f:
        json.dump(content_data, f, indent=2)
    os.chmod(content_path, 0o600)

    revision = {
        "revision_id": rev_id,
        "parent_revision_id": None,
        "timestamp": timestamp,
        "author": author,
        "diff": "",
        "integrity": {"checksum": checksum, "signature": signature},
        "message": "Initial creation",
    }

    history_dir = os.path.join(note_dir, "history")
    fs.makedirs(history_dir)
    os.chmod(history_dir, 0o700)

    rev_path = os.path.join(history_dir, f"{rev_id}.json")
    with fs.open(rev_path, "w") as f:
        json.dump(revision, f, indent=2)
    os.chmod(rev_path, 0o600)

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
    index_path = os.path.join(history_dir, "index.json")
    with fs.open(index_path, "w") as f:
        json.dump(history_index, f, indent=2)
    os.chmod(index_path, 0o600)

    # Create meta.json
    # Extract title from first H1 or use note_id
    title = note_id
    for line in content.splitlines():
        if line.startswith("# "):
            title = line[2:].strip()
            break

    meta = {
        "id": note_id,
        "workspace_id": os.path.basename(ws_path_str),  # Infer workspace_id
        "title": title,
        "class": parsed["frontmatter"].get("class"),
        "tags": parsed["frontmatter"].get("tags", []),
        "links": [],
        "canvas_position": {},
        "created_at": timestamp,
        "updated_at": timestamp,
        "integrity": {"checksum": checksum, "signature": signature},
    }

    meta_path = os.path.join(note_dir, "meta.json")
    with fs.open(meta_path, "w") as f:
        json.dump(meta, f, indent=2)
    os.chmod(meta_path, 0o600)


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
    ws_path_str = str(workspace_path)
    fs = fsspec.filesystem("file")

    note_dir = os.path.join(ws_path_str, "notes", note_id)

    if not fs.exists(note_dir):
        raise FileNotFoundError(f"Note {note_id} not found")

    # Check parent revision from content.json (as per spec)
    content_path = os.path.join(note_dir, "content.json")
    with fs.open(content_path, "r") as f:
        current_content_data = json.load(f)

    if current_content_data["revision_id"] != parent_revision_id:
        raise RevisionMismatchError(
            f"Expected {current_content_data['revision_id']}, got {parent_revision_id}"
        )

    # Parse content
    parsed = _parse_markdown(content)
    provider = integrity_provider or IntegrityProvider.for_workspace(workspace_path)

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

    with fs.open(content_path, "w") as f:
        json.dump(content_data, f, indent=2)
    os.chmod(content_path, 0o600)

    revision = {
        "revision_id": rev_id,
        "parent_revision_id": parent_revision_id,
        "timestamp": timestamp,
        "author": author,
        "diff": diff_text,
        "integrity": {"checksum": checksum, "signature": signature},
        "message": "Update",
    }

    history_dir = os.path.join(note_dir, "history")
    rev_path = os.path.join(history_dir, f"{rev_id}.json")
    with fs.open(rev_path, "w") as f:
        json.dump(revision, f, indent=2)
    os.chmod(rev_path, 0o600)

    # Update history index
    index_path = os.path.join(history_dir, "index.json")
    with fs.open(index_path, "r") as f:
        history_index = json.load(f)

    history_index["revisions"].append(
        {
            "revision_id": rev_id,
            "timestamp": timestamp,
            "checksum": checksum,
            "signature": signature,
        }
    )

    with fs.open(index_path, "w") as f:
        json.dump(history_index, f, indent=2)
    os.chmod(index_path, 0o600)

    # Update meta.json
    meta_path = os.path.join(note_dir, "meta.json")
    with fs.open(meta_path, "r") as f:
        meta = json.load(f)

    # Update title if changed
    title = meta["title"]
    for line in content.splitlines():
        if line.startswith("# "):
            title = line[2:].strip()
            break

    meta["title"] = title
    meta["updated_at"] = timestamp
    meta["class"] = parsed["frontmatter"].get("class", meta.get("class"))
    meta["tags"] = parsed["frontmatter"].get("tags", meta.get("tags", []))
    meta["integrity"] = {"checksum": checksum, "signature": signature}

    with fs.open(meta_path, "w") as f:
        json.dump(meta, f, indent=2)
    os.chmod(meta_path, 0o600)
