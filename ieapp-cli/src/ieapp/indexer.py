"""Live indexer utilities for projecting Markdown into structured caches."""

from __future__ import annotations

import contextlib
import json
import re
import time
from collections import Counter
from collections.abc import Callable
from typing import Any

import fsspec
import yaml

FRONTMATTER_PATTERN = re.compile(r"^---\s*\n(.*?)\n---\s*\n", re.DOTALL)
HEADER_PATTERN = re.compile(r"^##\s+(.+)$")


def _extract_frontmatter(content: str) -> tuple[dict[str, Any], str]:
    """Return parsed frontmatter and the remainder of the Markdown body."""
    match = FRONTMATTER_PATTERN.match(content)
    if not match:
        return {}, content

    frontmatter_text = match.group(1)
    try:
        parsed = yaml.safe_load(frontmatter_text) or {}
    except yaml.YAMLError:
        parsed = {}

    if not isinstance(parsed, dict):
        parsed = {}

    return parsed, content[match.end() :]


def _extract_sections(body: str) -> dict[str, str]:
    """Extract H2 sections as properties while respecting header boundaries."""
    sections: dict[str, str] = {}
    current_key: str | None = None
    buffer: list[str] = []

    for line in body.splitlines():
        header_match = HEADER_PATTERN.match(line)
        if header_match:
            if current_key is not None:
                sections[current_key] = "\n".join(buffer).strip()
            current_key = header_match.group(1).strip()
            buffer = []
            continue

        if line.startswith("#"):
            if current_key is not None:
                sections[current_key] = "\n".join(buffer).strip()
                current_key = None
                buffer = []
            continue

        if current_key is not None:
            buffer.append(line)

    if current_key is not None:
        sections[current_key] = "\n".join(buffer).strip()

    return sections


def extract_properties(content: str) -> dict[str, Any]:
    """Extract properties from Markdown frontmatter and H2 sections."""
    frontmatter, body = _extract_frontmatter(content)
    sections = _extract_sections(body)
    merged = dict(frontmatter)
    merged.update({key: value for key, value in sections.items() if value})
    return merged


def validate_properties(properties: dict, schema: dict) -> list[dict]:
    """Validate extracted properties against a schema definition."""
    warnings = []
    fields = schema.get("fields", {})

    for field_name, field_def in fields.items():
        if (
            field_def.get("required", False)
            and not properties.get(field_name, "").strip()
        ):
            warnings.append(
                {
                    "code": "missing_field",
                    "field": field_name,
                    "message": f"Missing required field: {field_name}",
                },
            )

    return warnings


def aggregate_stats(notes: dict[str, dict[str, Any]]) -> dict[str, Any]:
    """Build aggregate statistics for class and tag usage."""
    class_stats: dict[str, dict[str, int]] = {}
    tag_counts: Counter[str] = Counter()
    uncategorized_count = 0

    for record in notes.values():
        note_class = record.get("class") or record.get("properties", {}).get("class")
        if note_class:
            class_entry = class_stats.setdefault(note_class, {"count": 0})
            class_entry["count"] += 1
        else:
            uncategorized_count += 1

        for tag in record.get("tags") or []:
            tag_counts[tag] += 1

    class_stats["_uncategorized"] = {"count": uncategorized_count}

    return {
        "note_count": len(notes),
        "class_stats": class_stats,
        "tag_counts": dict(tag_counts),
    }


class Indexer:
    """Live indexer that projects Markdown notes into cached JSON views."""

    def __init__(
        self,
        workspace_path: str,
        fs: fsspec.AbstractFileSystem | None = None,
    ) -> None:
        """Initialize the indexer with the workspace root and filesystem."""
        self.workspace_path = workspace_path.rstrip("/")
        self.fs = fs or fsspec.filesystem("file")

    def run_once(self) -> None:
        """Build the structured cache and stats once."""
        notes_path = f"{self.workspace_path}/notes"
        index_path = f"{self.workspace_path}/index/index.json"
        stats_path = f"{self.workspace_path}/index/stats.json"
        schemas_path = f"{self.workspace_path}/schemas"

        self.fs.makedirs(f"{self.workspace_path}/index", exist_ok=True)

        schemas = self._load_schemas(schemas_path)
        index_notes = self._collect_notes(notes_path, schemas)
        stats_data = aggregate_stats(index_notes)

        index_payload = {
            "notes": index_notes,
            "class_stats": stats_data["class_stats"],
        }

        with self.fs.open(index_path, "w") as f:
            json.dump(index_payload, f, indent=2)

        stats_payload = {
            **stats_data,
            "last_indexed": time.time(),
        }
        with self.fs.open(stats_path, "w") as f:
            json.dump(stats_payload, f, indent=2)

    def _load_schemas(self, schemas_path: str) -> dict[str, dict[str, Any]]:
        """Load schema definitions from the workspace."""
        schemas: dict[str, dict[str, Any]] = {}
        if not self.fs.exists(schemas_path):
            return schemas

        for schema_file in self.fs.glob(f"{schemas_path}/*.json"):
            class_name = schema_file.split("/")[-1].removesuffix(".json")
            with self.fs.open(schema_file, "r") as handle:
                with contextlib.suppress(json.JSONDecodeError):
                    schemas[class_name] = json.load(handle)

        return schemas

    def _collect_notes(
        self,
        notes_path: str,
        schemas: dict[str, dict[str, Any]],
    ) -> dict[str, dict[str, Any]]:
        """Collect structured records for every note directory."""
        if not self.fs.exists(notes_path):
            return {}

        note_dirs = self.fs.ls(notes_path, detail=False)
        records: dict[str, dict[str, Any]] = {}

        for note_dir in note_dirs:
            note_id = note_dir.split("/")[-1]
            record = self._build_record(note_dir, note_id, schemas)
            if record is not None:
                records[note_id] = record

        return records

    def _build_record(
        self,
        note_dir: str,
        note_id: str,
        schemas: dict[str, dict[str, Any]],
    ) -> dict[str, Any] | None:
        """Build a single index record, returning ``None`` on decode errors."""
        content_path = f"{note_dir}/content.json"
        meta_path = f"{note_dir}/meta.json"

        if not self.fs.exists(content_path):
            return None

        with self.fs.open(content_path, "r") as handle:
            try:
                content_json = json.load(handle)
            except json.JSONDecodeError:
                return None

        markdown = content_json.get("markdown", "")
        properties = extract_properties(markdown)

        meta_json: dict[str, Any] = {}
        if self.fs.exists(meta_path):
            with self.fs.open(meta_path, "r") as meta_handle:
                with contextlib.suppress(json.JSONDecodeError):
                    meta_json = json.load(meta_handle)

        note_class = (
            meta_json.get("class")
            or properties.get("class")
            or content_json.get("frontmatter", {}).get("class")
        )

        warnings: list[dict[str, Any]] = []
        if note_class and note_class in schemas:
            warnings = validate_properties(properties, schemas[note_class])

        return {
            "id": note_id,
            "title": meta_json.get("title", note_id),
            "class": note_class,
            "updated_at": meta_json.get("updated_at"),
            "workspace_id": meta_json.get(
                "workspace_id",
                self.workspace_path.split("/")[-1],
            ),
            "properties": properties,
            "tags": meta_json.get("tags", []),
            "links": meta_json.get("links", []),
            "canvas_position": meta_json.get("canvas_position", {}),
            "checksum": (meta_json.get("integrity") or {}).get("checksum"),
            "validation_warnings": warnings,
        }

    def watch(
        self,
        wait_for_changes: Callable[[Callable[[], None]], None],
        *,
        on_error: Callable[[Exception], None] | None = None,
    ) -> None:
        """Run the indexer whenever ``wait_for_changes`` signals updates."""

        def _run_once() -> None:
            self.run_once()

        try:
            wait_for_changes(_run_once)
        except Exception as exc:
            if on_error:
                on_error(exc)
            else:
                raise


def _matches_filters(note: dict[str, Any], filters: dict[str, Any]) -> bool:
    """Return ``True`` when ``note`` satisfies ``filters``."""
    for key, expected in filters.items():
        note_value = note.get(key)
        if note_value is None:
            note_value = note.get("properties", {}).get(key)

        if isinstance(expected, dict):
            msg = (
                "Structured operators (e.g., $gt) are not implemented for the "
                "local query helper yet."
            )
            raise NotImplementedError(msg)

        if note_value != expected:
            return False

    return True


def query_index(
    workspace_path: str,
    filter_dict: dict[str, Any] | None,
    fs: fsspec.AbstractFileSystem | None = None,
) -> list[dict[str, Any]]:
    """Return note records from the cached index that satisfy ``filter_dict``."""
    fs = fs or fsspec.filesystem("file")
    index_path = f"{workspace_path.rstrip('/')}/index/index.json"

    if not fs.exists(index_path):
        return []

    with fs.open(index_path, "r") as handle:
        try:
            index_data = json.load(handle)
        except json.JSONDecodeError:
            return []

    notes = index_data.get("notes", {})
    if not filter_dict:
        return list(notes.values())

    return [note for note in notes.values() if _matches_filters(note, filter_dict)]
