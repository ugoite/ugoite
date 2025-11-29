import re
import yaml
import fsspec
import json
from typing import Any


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


def aggregate_stats(index_data: dict[str, Any]) -> dict[str, Any]:
    """
    Aggregates statistics from the index data.
    """
    class_stats: dict[str, dict[str, int]] = {}
    stats: dict[str, Any] = {"total_notes": len(index_data), "class_stats": class_stats}

    uncategorized_count = 0

    for _, properties in index_data.items():
        note_class = properties.get("class")

        if note_class:
            if note_class not in class_stats:
                class_stats[note_class] = {"count": 0}
            class_stats[note_class]["count"] += 1
        else:
            uncategorized_count += 1

    class_stats["_uncategorized"] = {"count": uncategorized_count}

    return stats


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

        index_data = {}

        # List notes
        if self.fs.exists(notes_path):
            note_dirs = self.fs.ls(notes_path, detail=False)

            for note_dir in note_dirs:
                note_id = note_dir.split("/")[-1]
                content_path = f"{note_dir}/content.json"

                if self.fs.exists(content_path):
                    with self.fs.open(content_path, "r") as f:
                        try:
                            content_json = json.load(f)
                            markdown = content_json.get("markdown", "")
                            properties = extract_properties(markdown)

                            # Validate
                            note_class = properties.get("class")
                            if note_class and note_class in schemas:
                                warnings = validate_properties(
                                    properties, schemas[note_class]
                                )
                                if warnings:
                                    properties["validation_warnings"] = warnings

                            index_data[note_id] = properties
                        except (json.JSONDecodeError, KeyError):
                            pass

        # Aggregate Stats
        stats_data = aggregate_stats(index_data)

        # Write Index
        with self.fs.open(index_path, "w") as f:
            json.dump(index_data, f, indent=2)

        # Write Stats
        with self.fs.open(stats_path, "w") as f:
            json.dump(stats_data, f, indent=2)


def query_index(
    workspace_path: str,
    filter_dict: dict[str, Any],
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """
    Queries the index for notes matching the filter.
    Returns a dictionary of note_id -> properties.
    """
    fs = fs or fsspec.filesystem("file")
    index_path = f"{workspace_path.rstrip('/')}/index/index.json"

    if not fs.exists(index_path):
        return {}

    with fs.open(index_path, "r") as f:
        try:
            index_data = json.load(f)
        except json.JSONDecodeError:
            return {}

    results = {}
    for note_id, properties in index_data.items():
        match = True
        for key, value in filter_dict.items():
            if properties.get(key) != value:
                match = False
                break
        if match:
            results[note_id] = properties

    return results
