import json
import os
import time
import hashlib
import uuid
import logging
from pathlib import Path
from typing import Union, Dict, Any
import fsspec

logger = logging.getLogger(__name__)


class NoteExistsError(Exception):
    pass


class RevisionMismatchError(Exception):
    pass


def _parse_markdown(content: str) -> Dict[str, Any]:
    """
    Parses markdown content to extract frontmatter and sections.
    """
    frontmatter = {}
    sections = {}

    # Extract Frontmatter
    if content.startswith("---\n"):
        try:
            parts = content.split("---\n", 2)
            if len(parts) >= 3:
                fm_str = parts[1]
                # Simple YAML parsing fallback since PyYAML might not be installed yet
                # In a real scenario, we'd add PyYAML to dependencies
                try:
                    import yaml

                    frontmatter = yaml.safe_load(fm_str) or {}
                except ImportError:
                    for line in fm_str.splitlines():
                        if ":" in line:
                            k, v = line.split(":", 1)
                            frontmatter[k.strip()] = v.strip()
        except Exception as e:
            logger.warning(f"Failed to parse frontmatter: {e}")

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


def _calculate_checksum(content: str) -> str:
    return hashlib.sha256(content.encode("utf-8")).hexdigest()


def _sign_revision(content: str) -> str:
    # Mock signature for Milestone 1
    return f"sig_{hashlib.md5(content.encode('utf-8')).hexdigest()}"


def create_note(workspace_path: Union[str, Path], note_id: str, content: str) -> None:
    ws_path_str = str(workspace_path)
    fs = fsspec.filesystem("file")

    note_dir = os.path.join(ws_path_str, "notes", note_id)

    if fs.exists(note_dir):
        raise NoteExistsError(f"Note {note_id} already exists")

    fs.makedirs(note_dir)

    # Parse content
    parsed = _parse_markdown(content)

    # Create initial revision
    rev_id = str(uuid.uuid4())
    timestamp = time.time()
    checksum = _calculate_checksum(content)
    signature = _sign_revision(content)

    # Create content.json
    content_data = {
        "revision_id": rev_id,
        "author": "user",  # Default author
        "markdown": content,
        "frontmatter": parsed["frontmatter"],
        "attachments": [],
        "computed": {},
    }

    with fs.open(os.path.join(note_dir, "content.json"), "w") as f:
        json.dump(content_data, f, indent=2)

    revision = {
        "revision_id": rev_id,
        "parent_revision_id": None,
        "timestamp": timestamp,
        "author": "user",
        "integrity": {"checksum": checksum, "signature": signature},
        "message": "Initial creation",
    }

    history_dir = os.path.join(note_dir, "history")
    fs.makedirs(history_dir)

    with fs.open(os.path.join(history_dir, f"{rev_id}.json"), "w") as f:
        json.dump(revision, f, indent=2)

    # Update history index
    history_index = {
        "note_id": note_id,
        "revisions": [{"revision_id": rev_id, "timestamp": timestamp}],
    }
    with fs.open(os.path.join(history_dir, "index.json"), "w") as f:
        json.dump(history_index, f, indent=2)

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

    with fs.open(os.path.join(note_dir, "meta.json"), "w") as f:
        json.dump(meta, f, indent=2)


def update_note(
    workspace_path: Union[str, Path],
    note_id: str,
    content: str,
    parent_revision_id: str,
) -> None:
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

    # Create new revision
    rev_id = str(uuid.uuid4())
    timestamp = time.time()
    checksum = _calculate_checksum(content)
    signature = _sign_revision(content)

    # Update content.json
    content_data = {
        "revision_id": rev_id,
        "author": "user",
        "markdown": content,
        "frontmatter": parsed["frontmatter"],
        "attachments": current_content_data.get("attachments", []),
        "computed": current_content_data.get("computed", {}),
    }

    with fs.open(content_path, "w") as f:
        json.dump(content_data, f, indent=2)

    revision = {
        "revision_id": rev_id,
        "parent_revision_id": parent_revision_id,
        "timestamp": timestamp,
        "author": "user",
        "integrity": {"checksum": checksum, "signature": signature},
        "message": "Update",
    }

    history_dir = os.path.join(note_dir, "history")
    with fs.open(os.path.join(history_dir, f"{rev_id}.json"), "w") as f:
        json.dump(revision, f, indent=2)

    # Update history index
    index_path = os.path.join(history_dir, "index.json")
    with fs.open(index_path, "r") as f:
        history_index = json.load(f)

    history_index["revisions"].append({"revision_id": rev_id, "timestamp": timestamp})

    with fs.open(index_path, "w") as f:
        json.dump(history_index, f, indent=2)

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
