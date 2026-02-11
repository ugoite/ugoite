"""Live indexer utilities for projecting Markdown into structured caches."""

from __future__ import annotations

import contextlib
import json
import re
from collections import Counter
from typing import TYPE_CHECKING, Annotated, Any, cast

import fsspec
import ugoite_core
import yaml
from pydantic import BeforeValidator

from .utils import (
    run_async,
    split_space_path,
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
    return run_async(ugoite_core.extract_properties, content)


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

    entry_form = record.get("form")
    if entry_form:
        tokens.update(_tokenize_text_for_index(entry_form))

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
    entries: dict[str, dict[str, Any]],
) -> dict[str, list[str]]:
    """Build an inverted index mapping terms to entry IDs.

    Args:
        entries: Dictionary mapping entry IDs to their record dictionaries.

    Returns:
        An inverted index mapping each token to a list of entry IDs.

    """
    inverted: dict[str, list[str]] = {}
    for entry_id, record in entries.items():
        for token in _tokenize_record_for_index(record):
            inverted.setdefault(token, [])
            if entry_id not in inverted[token]:
                inverted[token].append(entry_id)

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
    entry_form: dict[str, Any],
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    """Validate and cast extracted properties against a entry_form definition.

    Returns:
        A tuple of (casted_properties, warnings).

    """
    casted, warnings = run_async(
        ugoite_core.validate_properties,
        json.dumps(properties),
        json.dumps(entry_form),
    )
    return casted, warnings


def aggregate_stats(entries: dict[str, dict[str, Any]]) -> dict[str, Any]:
    """Build aggregate statistics for form and tag usage.

    Args:
        entries: Dictionary mapping entry IDs to their record dictionaries.

    Returns:
        A dictionary containing entry_count, form_stats, and tag_counts.

    """
    form_stats: dict[str, dict[str, Any]] = {}
    tag_counts: Counter[str] = Counter()
    uncategorized_count = 0

    for record in entries.values():
        entry_form = record.get("form") or record.get("properties", {}).get("form")
        if entry_form:
            form_entry = form_stats.setdefault(
                entry_form,
                {"count": 0, "fields": Counter()},
            )
            form_entry["count"] += 1

            # Count field usage
            properties = record.get("properties", {})
            for key in properties:
                cast("Counter", form_entry["fields"])[key] += 1
        else:
            uncategorized_count += 1

        for tag in record.get("tags") or []:
            tag_counts[tag] += 1

    # Convert Counters to dicts for JSON serialization
    for entry in form_stats.values():
        if "fields" in entry and isinstance(entry["fields"], Counter):
            entry["fields"] = dict(entry["fields"])

    form_stats["_uncategorized"] = {"count": uncategorized_count}

    return {
        "entry_count": len(entries),
        "form_stats": form_stats,
        "tag_counts": dict(tag_counts),
    }


class Indexer:
    """Live indexer that projects Markdown entries into cached JSON views."""

    def __init__(
        self,
        space_path: str,
        fs: fsspec.AbstractFileSystem | None = None,
    ) -> None:
        """Initialize the indexer with the space root and filesystem.

        Args:
            space_path: Path to the space directory.
            fs: Optional filesystem implementation. Defaults to local filesystem.

        """
        self.space_path = space_path.rstrip("/")
        self._use_fsspec = fs is not None
        self.fs = fs or fsspec.filesystem("file")

    def update_entry_index(self, entry_id: str) -> None:
        """Incrementally update the index for a single entry.

        Args:
            entry_id: The ID of the entry to update.

        """
        if self._use_fsspec:
            self.run_once()
            return

        root_path, space_id = split_space_path(self.space_path)
        config = storage_config_from_root(root_path, self.fs)
        run_async(ugoite_core.update_entry_index, config, space_id, entry_id)

    def run_once(self) -> None:
        """Build the structured cache and stats once.

        Loads forms, collects entry data, generates an inverted index for search,
        and persists index.json, inverted_index.json, and stats.json to the space.
        """
        root_path, space_id = split_space_path(self.space_path)
        config = storage_config_from_root(root_path, self.fs)
        run_async(ugoite_core.reindex_all, config, space_id)

    def _build_inverted_index(
        self,
        entries: dict[str, dict[str, Any]],
    ) -> dict[str, list[str]]:
        """Build an inverted index mapping terms to entry IDs.

        Tokenizes text content (title, properties, tags) from each entry and
        creates posting lists for keyword search support.

        Args:
            entries: Dictionary mapping entry IDs to their record dictionaries.

        Returns:
            An inverted index mapping each token to a list of entry IDs containing
            that token.

        """
        inverted: dict[str, list[str]] = {}
        for entry_id, record in entries.items():
            tokens = self._tokenize_record(record)
            for token in tokens:
                if token not in inverted:
                    inverted[token] = []
                if entry_id not in inverted[token]:
                    inverted[token].append(entry_id)

        return inverted

    def _tokenize_record(self, record: dict[str, Any]) -> set[str]:
        """Extract lowercase tokens from a entry record for indexing.

        Args:
            record: The entry record dictionary containing title, tags, form,
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

        # Tokenize form
        entry_form = record.get("form")
        if entry_form:
            tokens.update(self._tokenize_text(entry_form))

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

    def _load_forms(self, forms_path: str) -> dict[str, dict[str, Any]]:
        """Load entry_form definitions from the space.

        Args:
            forms_path: Path to the forms directory.

        Returns:
            A dictionary mapping form names to their entry_form definitions.

        """
        forms: dict[str, dict[str, Any]] = {}
        if not self.fs.exists(forms_path):
            return forms

        for entry_form_file in self.fs.glob(f"{forms_path}/*.json"):
            form_name = entry_form_file.split("/")[-1].removesuffix(".json")
            with (
                contextlib.suppress(json.JSONDecodeError),
                self.fs.open(
                    entry_form_file,
                    "r",
                ) as handle,
            ):
                forms[form_name] = json.load(handle)

        return forms

    def _collect_entries(
        self,
        entries_path: str,
        forms: dict[str, dict[str, Any]],
    ) -> dict[str, dict[str, Any]]:
        """Collect structured records for every entry directory.

        Args:
            entries_path: Path to the entries directory.
            forms: Dictionary of entry_form definitions keyed by form name.

        Returns:
            A dictionary mapping entry IDs to their structured records.

        """
        if not self.fs.exists(entries_path):
            return {}

        entry_dirs = self.fs.ls(entries_path, detail=False)
        records: dict[str, dict[str, Any]] = {}

        for entry_dir in entry_dirs:
            entry_id = entry_dir.split("/")[-1]
            record = self._build_record(entry_dir, entry_id, forms)
            if record is not None:
                records[entry_id] = record

        return records

    def _build_record(
        self,
        entry_dir: str,
        entry_id: str,
        forms: dict[str, dict[str, Any]],
    ) -> dict[str, Any] | None:
        """Build a single index record, returning ``None`` on decode errors.

        Args:
            entry_dir: Path to the entry directory.
            entry_id: The entry's unique identifier.
            forms: Dictionary of entry_form definitions keyed by form name.

        Returns:
            A structured record dictionary, or None if the entry cannot be read
            or parsed.

        """
        content_path = f"{entry_dir}/content.json"
        meta_path = f"{entry_dir}/meta.json"

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

        entry_form = (
            meta_json.get("form")
            or properties.get("form")
            or content_json.get("frontmatter", {}).get("form")
        )

        warnings: list[dict[str, Any]] = []
        if entry_form and entry_form in forms:
            properties, warnings = validate_properties(properties, forms[entry_form])

        # Calculate word count (simple whitespace split)
        word_count = len(markdown.split())

        return {
            "id": entry_id,
            "title": meta_json.get("title", entry_id),
            "form": entry_form,
            "updated_at": meta_json.get("updated_at"),
            "space_id": meta_json.get(
                "space_id",
                self.space_path.split("/")[-1],
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


def _matches_filters(entry: dict[str, Any], filters: dict[str, Any]) -> bool:
    """Return ``True`` when ``entry`` satisfies ``filters``.

    Args:
        entry: The entry record dictionary to check.
        filters: Dictionary of filter criteria where keys are field names and
            values are expected values.

    Returns:
        True if the entry satisfies all filter criteria, False otherwise.

    """
    for key, expected in filters.items():
        entry_value = entry.get(key)
        if entry_value is None:
            entry_value = entry.get("properties", {}).get(key)

        if isinstance(expected, dict):
            msg = (
                "Structured operators (e.g., $gt) are not implemented for the "
                "local query helper yet."
            )
            raise NotImplementedError(msg)

        # Handle list membership (e.g., tags)
        if key == "tag" and "tags" in entry:
            if expected not in (entry.get("tags") or []):
                return False
            continue

        if isinstance(entry_value, list):
            if expected not in entry_value:
                return False
        elif entry_value != expected:
            return False

    return True


def query_index(
    space_path: str,
    filter_dict: dict[str, Any] | None,
    fs: fsspec.AbstractFileSystem | None = None,
) -> list[dict[str, Any]]:
    """Return entry records from the cached index that satisfy ``filter_dict``.

    Args:
        space_path: Path to the space directory.
        filter_dict: Dictionary of filter criteria, or None to return all entries.
        fs: Optional filesystem implementation. Defaults to local filesystem.

    Returns:
        A list of entry records that match the filter criteria.

    """
    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path, fs)
    payload = json.dumps(filter_dict or {})
    return run_async(ugoite_core.query_index, config, space_id, payload)


def create_sql_session(
    space_path: str,
    sql: str,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Create a SQL session for the given query."""
    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path, fs)
    return run_async(ugoite_core.create_sql_session, config, space_id, sql)


def get_sql_session_rows(
    space_path: str,
    session_id: str,
    offset: int = 0,
    limit: int = 50,
    fs: fsspec.AbstractFileSystem | None = None,
) -> dict[str, Any]:
    """Retrieve paged rows from a SQL session."""
    root_path, space_id = split_space_path(space_path)
    config = storage_config_from_root(root_path, fs)
    return run_async(
        ugoite_core.get_sql_session_rows,
        config,
        space_id,
        session_id,
        offset,
        limit,
    )
