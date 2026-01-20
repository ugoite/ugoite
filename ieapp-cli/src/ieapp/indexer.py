"""Live indexer utilities for projecting Markdown into structured caches."""

from __future__ import annotations

import contextlib
import json
import re
from collections import Counter
from typing import TYPE_CHECKING, Annotated, Any, cast

import fsspec
import ieapp_core
import yaml
from pydantic import BeforeValidator

from .utils import (
    run_async,
    split_workspace_path,
    storage_config_from_root,
)

if TYPE_CHECKING:
    from collections.abc import Callable

FRONTMATTER_PATTERN = re.compile(r"^---\s*\n(.*?)\n---\s*\n", re.DOTALL)
HEADER_PATTERN = re.compile(r"^##\s+(.+)$")


def _extract_frontmatter(content: str) -> tuple[dict[str, Any], str]:
    """Return parsed frontmatter and the remainder of the Markdown body.

    Args:
        content: The full Markdown content including potential frontmatter.

    Returns:
        A tuple of (frontmatter_dict, remaining_body). If no frontmatter is found,
        returns an empty dict and the original content.

    """
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
    """Extract H2 sections as properties while respecting header boundaries.

    Args:
        body: The Markdown body text (without frontmatter).

    Returns:
        A dictionary mapping H2 section headers to their content.

    """
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
    """Extract properties from Markdown frontmatter and H2 sections.

    Args:
        content: The Markdown content to extract properties from.

    Returns:
        A dictionary of extracted properties with H2 sections overriding
        frontmatter values.

    """
    return run_async(ieapp_core.extract_properties, content)


def compute_word_count(content: str) -> int:
    """Return the word count for markdown content.

    Args:
        content: Markdown content string.

    Returns:
        The number of whitespace-delimited tokens.

    """
    return len(content.split())


def _tokenize_text_for_index(text: str) -> set[str]:
    return {
        word.lower()
        for word in re.findall(r"\w+", text)
        if len(word) > 1 and not word.isnumeric()
    }


def _tokenize_record_for_index(record: dict[str, Any]) -> set[str]:
    tokens: set[str] = set()
    tokens.update(_tokenize_text_for_index(record.get("title", "")))

    for tag in record.get("tags") or []:
        tokens.update(_tokenize_text_for_index(tag))

    note_class = record.get("class")
    if note_class:
        tokens.update(_tokenize_text_for_index(note_class))

    properties = record.get("properties") or {}
    for key, value in properties.items():
        tokens.update(_tokenize_text_for_index(key))
        if isinstance(value, str):
            tokens.update(_tokenize_text_for_index(value))
        elif isinstance(value, list):
            for item in value:
                if isinstance(item, str):
                    tokens.update(_tokenize_text_for_index(item))

    return tokens


def build_inverted_index(
    notes: dict[str, dict[str, Any]],
) -> dict[str, list[str]]:
    """Build an inverted index mapping terms to note IDs.

    Args:
        notes: Dictionary mapping note IDs to their record dictionaries.

    Returns:
        An inverted index mapping each token to a list of note IDs.

    """
    inverted: dict[str, list[str]] = {}
    for note_id, record in notes.items():
        for token in _tokenize_record_for_index(record):
            inverted.setdefault(token, [])
            if note_id not in inverted[token]:
                inverted[token].append(note_id)

    return inverted


def parse_markdown_list(v: Any) -> Any:
    """Parse markdown list syntax into a python list.

    Args:
        v: The value to parse, typically a string with markdown list syntax.

    Returns:
        A list of items if v is a string with markdown list syntax, otherwise
        v unchanged.

    """
    if isinstance(v, str):
        items = []
        for raw_line in v.splitlines():
            line = raw_line.strip()
            if line.startswith(("- ", "* ")):
                items.append(line[2:])
            elif line:
                items.append(line)
        return items
    return v


MarkdownList = Annotated[list[str], BeforeValidator(parse_markdown_list)]


def validate_properties(
    properties: dict[str, Any],
    note_class: dict[str, Any],
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    """Validate and cast extracted properties against a note_class definition.

    Returns:
        A tuple of (casted_properties, warnings).

    """
    casted, warnings = run_async(
        ieapp_core.validate_properties,
        json.dumps(properties),
        json.dumps(note_class),
    )
    return casted, warnings


def aggregate_stats(notes: dict[str, dict[str, Any]]) -> dict[str, Any]:
    """Build aggregate statistics for class and tag usage.

    Args:
        notes: Dictionary mapping note IDs to their record dictionaries.

    Returns:
        A dictionary containing note_count, class_stats, and tag_counts.

    """
    class_stats: dict[str, dict[str, Any]] = {}
    tag_counts: Counter[str] = Counter()
    uncategorized_count = 0

    for record in notes.values():
        note_class = record.get("class") or record.get("properties", {}).get("class")
        if note_class:
            class_entry = class_stats.setdefault(
                note_class,
                {"count": 0, "fields": Counter()},
            )
            class_entry["count"] += 1

            # Count field usage
            properties = record.get("properties", {})
            for key in properties:
                cast("Counter", class_entry["fields"])[key] += 1
        else:
            uncategorized_count += 1

        for tag in record.get("tags") or []:
            tag_counts[tag] += 1

    # Convert Counters to dicts for JSON serialization
    for entry in class_stats.values():
        if "fields" in entry and isinstance(entry["fields"], Counter):
            entry["fields"] = dict(entry["fields"])

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
        """Initialize the indexer with the workspace root and filesystem.

        Args:
            workspace_path: Path to the workspace directory.
            fs: Optional filesystem implementation. Defaults to local filesystem.

        """
        self.workspace_path = workspace_path.rstrip("/")
        self._use_fsspec = fs is not None
        self.fs = fs or fsspec.filesystem("file")

    def update_note_index(self, note_id: str) -> None:
        """Incrementally update the index for a single note.

        Args:
            note_id: The ID of the note to update.

        """
        if self._use_fsspec:
            self.run_once()
            return

        root_path, workspace_id = split_workspace_path(self.workspace_path)
        config = storage_config_from_root(root_path, self.fs)
        run_async(ieapp_core.update_note_index, config, workspace_id, note_id)

    def run_once(self) -> None:
        """Build the structured cache and stats once.

        Loads classes, collects note data, generates an inverted index for search,
        and persists index.json, inverted_index.json, and stats.json to the workspace.
        """
        root_path, workspace_id = split_workspace_path(self.workspace_path)
        config = storage_config_from_root(root_path, self.fs)
        run_async(ieapp_core.reindex_all, config, workspace_id)

    def _build_inverted_index(
        self,
        notes: dict[str, dict[str, Any]],
    ) -> dict[str, list[str]]:
        """Build an inverted index mapping terms to note IDs.

        Tokenizes text content (title, properties, tags) from each note and
        creates posting lists for keyword search support.

        Args:
            notes: Dictionary mapping note IDs to their record dictionaries.

        Returns:
            An inverted index mapping each token to a list of note IDs containing
            that token.

        """
        inverted: dict[str, list[str]] = {}
        for note_id, record in notes.items():
            tokens = self._tokenize_record(record)
            for token in tokens:
                if token not in inverted:
                    inverted[token] = []
                if note_id not in inverted[token]:
                    inverted[token].append(note_id)

        return inverted

    def _tokenize_record(self, record: dict[str, Any]) -> set[str]:
        """Extract lowercase tokens from a note record for indexing.

        Args:
            record: The note record dictionary containing title, tags, class,
                and properties.

        Returns:
            A set of unique lowercase tokens extracted from the record.

        """
        tokens: set[str] = set()

        # Tokenize title
        title = record.get("title", "")
        tokens.update(self._tokenize_text(title))

        # Tokenize tags
        for tag in record.get("tags") or []:
            tokens.update(self._tokenize_text(tag))

        # Tokenize class
        note_class = record.get("class")
        if note_class:
            tokens.update(self._tokenize_text(note_class))

        # Tokenize properties (both keys and string values)
        properties = record.get("properties") or {}
        for key, value in properties.items():
            tokens.update(self._tokenize_text(key))
            if isinstance(value, str):
                tokens.update(self._tokenize_text(value))
            elif isinstance(value, list):
                for item in value:
                    if isinstance(item, str):
                        tokens.update(self._tokenize_text(item))

        return tokens

    @staticmethod
    def _tokenize_text(text: str) -> set[str]:
        """Split text into lowercase alphabetic tokens.

        Args:
            text: The text to tokenize.

        Returns:
            A set of unique lowercase tokens (alphanumeric sequences longer than
            1 character).

        """
        # Simple word tokenization: extract alphanumeric sequences
        return {
            word.lower()
            for word in re.findall(r"\w+", text)
            if len(word) > 1 and not word.isnumeric()
        }

    def _load_classes(self, classes_path: str) -> dict[str, dict[str, Any]]:
        """Load note_class definitions from the workspace.

        Args:
            classes_path: Path to the classes directory.

        Returns:
            A dictionary mapping class names to their note_class definitions.

        """
        classes: dict[str, dict[str, Any]] = {}
        if not self.fs.exists(classes_path):
            return classes

        for note_class_file in self.fs.glob(f"{classes_path}/*.json"):
            class_name = note_class_file.split("/")[-1].removesuffix(".json")
            with (
                contextlib.suppress(json.JSONDecodeError),
                self.fs.open(
                    note_class_file,
                    "r",
                ) as handle,
            ):
                classes[class_name] = json.load(handle)

        return classes

    def _collect_notes(
        self,
        notes_path: str,
        classes: dict[str, dict[str, Any]],
    ) -> dict[str, dict[str, Any]]:
        """Collect structured records for every note directory.

        Args:
            notes_path: Path to the notes directory.
            classes: Dictionary of note_class definitions keyed by class name.

        Returns:
            A dictionary mapping note IDs to their structured records.

        """
        if not self.fs.exists(notes_path):
            return {}

        note_dirs = self.fs.ls(notes_path, detail=False)
        records: dict[str, dict[str, Any]] = {}

        for note_dir in note_dirs:
            note_id = note_dir.split("/")[-1]
            record = self._build_record(note_dir, note_id, classes)
            if record is not None:
                records[note_id] = record

        return records

    def _build_record(
        self,
        note_dir: str,
        note_id: str,
        classes: dict[str, dict[str, Any]],
    ) -> dict[str, Any] | None:
        """Build a single index record, returning ``None`` on decode errors.

        Args:
            note_dir: Path to the note directory.
            note_id: The note's unique identifier.
            classes: Dictionary of note_class definitions keyed by class name.

        Returns:
            A structured record dictionary, or None if the note cannot be read
            or parsed.

        """
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
            with (
                contextlib.suppress(json.JSONDecodeError),
                self.fs.open(
                    meta_path,
                    "r",
                ) as meta_handle,
            ):
                meta_json = json.load(meta_handle)

        note_class = (
            meta_json.get("class")
            or properties.get("class")
            or content_json.get("frontmatter", {}).get("class")
        )

        warnings: list[dict[str, Any]] = []
        if note_class and note_class in classes:
            properties, warnings = validate_properties(properties, classes[note_class])

        # Calculate word count (simple whitespace split)
        word_count = len(markdown.split())

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
            "word_count": word_count,
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
        """Run the indexer whenever ``wait_for_changes`` signals updates.

        Args:
            wait_for_changes: A callable that invokes the provided callback when
                changes occur.
            on_error: Optional error handler callback. If not provided, exceptions
                are re-raised.

        """

        def _run_once() -> None:
            try:
                self.run_once()
            except Exception as exc:
                if on_error:
                    on_error(exc)
                else:
                    raise

        # Watch loop
        wait_for_changes(_run_once)


def _matches_filters(note: dict[str, Any], filters: dict[str, Any]) -> bool:
    """Return ``True`` when ``note`` satisfies ``filters``.

    Args:
        note: The note record dictionary to check.
        filters: Dictionary of filter criteria where keys are field names and
            values are expected values.

    Returns:
        True if the note satisfies all filter criteria, False otherwise.

    """
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

        # Handle list membership (e.g., tags)
        if key == "tag" and "tags" in note:
            if expected not in (note.get("tags") or []):
                return False
            continue

        if isinstance(note_value, list):
            if expected not in note_value:
                return False
        elif note_value != expected:
            return False

    return True


def query_index(
    workspace_path: str,
    filter_dict: dict[str, Any] | None,
    fs: fsspec.AbstractFileSystem | None = None,
) -> list[dict[str, Any]]:
    """Return note records from the cached index that satisfy ``filter_dict``.

    Args:
        workspace_path: Path to the workspace directory.
        filter_dict: Dictionary of filter criteria, or None to return all notes.
        fs: Optional filesystem implementation. Defaults to local filesystem.

    Returns:
        A list of note records that match the filter criteria.

    """
    root_path, workspace_id = split_workspace_path(workspace_path)
    config = storage_config_from_root(root_path, fs)
    payload = json.dumps(filter_dict or {})
    return run_async(ieapp_core.query_index, config, workspace_id, payload)
