import json
import re
import time
from collections import Counter
from typing import Any, Callable

import fsspec
import yaml


def extract_properties(content: str) -> dict[str, Any]:
    """
    Extracts properties from Markdown content, merging Frontmatter and H2 sections.

    Precedence: Section > Frontmatter
    """
    properties: dict[str, Any] = {}

    # 1. Extract Frontmatter
    frontmatter_match = re.match(r"^---\s*\n(.*?)\n---\s*\n", content, re.DOTALL)
    body_start_index = 0

    if frontmatter_match:
        frontmatter_text = frontmatter_match.group(1)
        try:
            frontmatter_data = yaml.safe_load(frontmatter_text)
            if isinstance(frontmatter_data, dict):
                properties.update(frontmatter_data)
        except yaml.YAMLError:
            pass  # Ignore invalid YAML for now
        body_start_index = frontmatter_match.end()

    body = content[body_start_index:]

    # 2. Extract H2 Sections
    # Regex to find ## Header and the following content
    # We look for lines starting with ## followed by space and the key
    # Then capture everything until the next line starting with # or end of string

    # Split by lines to process line by line for robustness
    lines = body.split("\n")
    current_key = None
    current_value_lines = []

    for line in lines:
        header_match = re.match(r"^##\s+(.+)$", line)
        if header_match:
            # Save previous section if exists
            if current_key:
                properties[current_key] = "\n".join(current_value_lines).strip()

            current_key = header_match.group(1).strip()
            current_value_lines = []
        elif line.startswith("# "):
            # H1 or other headers might reset context, but spec says "H2 headers"
            # If we encounter H1, we probably should stop the previous H2 context?
            # Spec says "System parses H2 headers as property keys."
            # It doesn't explicitly say H1 stops it, but usually headers delineate sections.
            # Let's assume any header stops the previous section, but only H2 are properties.
            if current_key:
                properties[current_key] = "\n".join(current_value_lines).strip()
                current_key = None
                current_value_lines = []
        else:
            if current_key is not None:
                current_value_lines.append(line)

    # Save the last section
    if current_key:
        properties[current_key] = "\n".join(current_value_lines).strip()

    return properties


def validate_properties(properties: dict, schema: dict) -> list[dict]:
    """
    Validates extracted properties against a Class schema.
    Returns a list of warning dictionaries.
    """
    warnings = []
    fields = schema.get("fields", {})

    for field_name, field_def in fields.items():
        if field_def.get("required", False):
            if field_name not in properties or not properties[field_name].strip():
                warnings.append(
                    {
                        "code": "missing_field",
                        "field": field_name,
                        "message": f"Missing required field: {field_name}",
                    }
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
    def __init__(
        self, workspace_path: str, fs: fsspec.AbstractFileSystem | None = None
    ):
        self.workspace_path = workspace_path.rstrip("/")
        self.fs = fs or fsspec.filesystem("file")

    def run_once(self):
        notes_path = f"{self.workspace_path}/notes"
        index_path = f"{self.workspace_path}/index/index.json"
        stats_path = f"{self.workspace_path}/index/stats.json"
        schemas_path = f"{self.workspace_path}/schemas"

        self.fs.makedirs(f"{self.workspace_path}/index", exist_ok=True)

        # Load Schemas
        schemas = {}
        if self.fs.exists(schemas_path):
            schema_files = self.fs.glob(f"{schemas_path}/*.json")
            for schema_file in schema_files:
                # schema_file is full path
                class_name = schema_file.split("/")[-1].replace(".json", "")
                with self.fs.open(schema_file, "r") as f:
                    try:
                        schemas[class_name] = json.load(f)
                    except json.JSONDecodeError:
                        pass

        index_notes: dict[str, dict[str, Any]] = {}

        # List notes
        if self.fs.exists(notes_path):
            note_dirs = self.fs.ls(notes_path, detail=False)

            for note_dir in note_dirs:
                note_id = note_dir.split("/")[-1]
                content_path = f"{note_dir}/content.json"
                meta_path = f"{note_dir}/meta.json"

                if self.fs.exists(content_path):
                    with self.fs.open(content_path, "r") as f:
                        try:
                            content_json = json.load(f)
                            markdown = content_json.get("markdown", "")
                            properties = extract_properties(markdown)

                            meta_json: dict[str, Any] = {}
                            if self.fs.exists(meta_path):
                                with self.fs.open(meta_path, "r") as meta_file:
                                    try:
                                        meta_json = json.load(meta_file)
                                    except json.JSONDecodeError:
                                        meta_json = {}

                            note_class = (
                                meta_json.get("class")
                                or properties.get("class")
                                or content_json.get("frontmatter", {}).get("class")
                            )

                            warnings: list[dict[str, Any]] = []
                            if note_class and note_class in schemas:
                                warnings = validate_properties(
                                    properties,
                                    schemas[note_class],
                                )

                            record = {
                                "id": note_id,
                                "title": meta_json.get("title", note_id),
                                "class": note_class,
                                "updated_at": meta_json.get("updated_at"),
                                "workspace_id": meta_json.get(
                                    "workspace_id", self.workspace_path.split("/")[-1]
                                ),
                                "properties": properties,
                                "tags": meta_json.get("tags", []),
                                "links": meta_json.get("links", []),
                                "canvas_position": meta_json.get(
                                    "canvas_position", {}
                                ),
                                "checksum": (meta_json.get("integrity") or {}).get(
                                    "checksum"
                                ),
                                "validation_warnings": warnings,
                            }

                            index_notes[note_id] = record
                        except (json.JSONDecodeError, KeyError):
                            pass

        # Aggregate Stats
        stats_data = aggregate_stats(index_notes)

        index_payload = {
            "notes": index_notes,
            "class_stats": stats_data["class_stats"],
        }

        # Write Index
        with self.fs.open(index_path, "w") as f:
            json.dump(index_payload, f, indent=2)

        # Write Stats
        stats_payload = {
            **stats_data,
            "last_indexed": time.time(),
        }
        with self.fs.open(stats_path, "w") as f:
            json.dump(stats_payload, f, indent=2)

    def watch(
        self,
        wait_for_changes: Callable[[Callable[[], None]], None],
        *,
        on_error: Callable[[Exception], None] | None = None,
    ) -> None:
        """Run the indexer whenever ``wait_for_changes`` signals updates.

        The provided callable is responsible for blocking or polling the
        filesystem. It receives a callback that should be invoked each time a
        change is detected. Tests can inject a synchronous stub to deterministically
        trigger the indexer without relying on timers or OS watchers.
        """

        def _run_once() -> None:
            self.run_once()

        try:
            wait_for_changes(_run_once)
        except Exception as exc:  # noqa: BLE001
            if on_error:
                on_error(exc)
            else:
                raise


def _matches_filters(note: dict[str, Any], filters: dict[str, Any]) -> bool:
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

    with fs.open(index_path, "r") as f:
        try:
            index_data = json.load(f)
        except json.JSONDecodeError:
            return []

    notes = index_data.get("notes", {})
    if not filter_dict:
        return list(notes.values())

    results = []
    for note in notes.values():
        if _matches_filters(note, filter_dict):
            results.append(note)

    return results
