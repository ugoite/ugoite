"""Render repository-managed release notes for stable, beta, and alpha channels."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
CHANGELOG_DIR = REPO_ROOT / "docs" / "version" / "changelog"


def _require_non_empty_string(
    mapping: dict[str, object],
    key: str,
    *,
    context: str,
) -> str:
    value = mapping.get(key)
    if not isinstance(value, str) or not value.strip():
        message = f"{context}.{key} must be a non-empty string"
        raise ValueError(message)
    return value.strip()


def _require_string_list(
    mapping: dict[str, object],
    key: str,
    *,
    context: str,
) -> list[str]:
    value = mapping.get(key)
    if not isinstance(value, list) or not value:
        message = f"{context}.{key} must be a non-empty list"
        raise ValueError(message)
    normalized = [
        item.strip() for item in value if isinstance(item, str) and item.strip()
    ]
    if len(normalized) != len(value):
        message = f"{context}.{key} must contain only non-empty strings"
        raise ValueError(message)
    return normalized


def _parse_folded(
    lines: list[str],
    start_index: int,
    *,
    indent: int,
) -> tuple[str, int]:
    parts: list[str] = []
    index = start_index
    prefix = " " * indent
    while index < len(lines):
        line = lines[index]
        if not line.strip():
            index += 1
            continue
        if not line.startswith(prefix):
            break
        parts.append(line[indent:].strip())
        index += 1
    if not parts:
        message = f"Expected folded block with indent {indent}"
        raise ValueError(message)
    return " ".join(parts), index


def _parse_list(
    lines: list[str],
    start_index: int,
) -> tuple[list[str], int]:
    items: list[str] = []
    index = start_index
    while index < len(lines):
        item_line = lines[index]
        if not item_line.strip():
            index += 1
            continue
        if not item_line.startswith("    - "):
            break
        items.append(item_line[6:].strip())
        index += 1
    return items, index


def _parse_release_notes(
    lines: list[str],
    start_index: int,
    *,
    path: Path,
) -> tuple[dict[str, object], int]:
    release_notes: dict[str, object] = {}
    index = start_index

    while index < len(lines):
        nested = lines[index]
        if not nested.strip():
            index += 1
            continue
        if not nested.startswith("  "):
            break

        stripped = nested[2:]
        if stripped.endswith(":"):
            key = stripped[:-1]
            items, index = _parse_list(lines, index + 1)
            release_notes[key] = items
            continue

        key, separator, value = stripped.partition(":")
        if separator != ":":
            relative_path = path.relative_to(REPO_ROOT)
            message = f"Unsupported nested line in {relative_path}: {nested}"
            raise ValueError(message)

        normalized_key = key.strip()
        normalized_value = value.strip()
        if normalized_value == ">":
            folded, index = _parse_folded(lines, index + 1, indent=4)
            release_notes[normalized_key] = folded
            continue

        release_notes[normalized_key] = normalized_value
        index += 1

    return release_notes, index


def _load_channel_document(path: Path) -> dict[str, object]:
    lines = path.read_text(encoding="utf-8").splitlines()
    document: dict[str, object] = {}
    index = 0

    while index < len(lines):
        raw_line = lines[index]
        if not raw_line.strip():
            index += 1
            continue

        if raw_line.startswith("release_notes:"):
            release_notes, index = _parse_release_notes(
                lines,
                index + 1,
                path=path,
            )
            document["release_notes"] = release_notes
            continue

        key, separator, value = raw_line.partition(":")
        if separator != ":":
            relative_path = path.relative_to(REPO_ROOT)
            message = f"Unsupported line in {relative_path}: {raw_line}"
            raise ValueError(message)

        normalized_key = key.strip()
        normalized_value = value.strip()
        if normalized_value == ">":
            folded, index = _parse_folded(lines, index + 1, indent=2)
            document[normalized_key] = folded
            continue

        document[normalized_key] = normalized_value
        index += 1

    return document


def _render_section(title: str, items: list[str]) -> str:
    bullets = "\n".join(f"- {item}" for item in items)
    return f"## {title}\n\n{bullets}"


def render_release_notes(*, channel: str, version: str) -> str:
    """Render markdown release notes for the requested release channel."""
    channel_path = CHANGELOG_DIR / f"{channel}.yaml"
    document = _load_channel_document(channel_path)
    context = str(channel_path.relative_to(REPO_ROOT))
    configured_channel = _require_non_empty_string(
        document,
        "channel",
        context=context,
    )
    if configured_channel != channel:
        message = f"{context} channel must be {channel!r} (got {configured_channel!r})"
        raise ValueError(message)

    title = _require_non_empty_string(document, "title", context=context)
    doc_path = _require_non_empty_string(document, "doc_path", context=context)
    full_doc_path = REPO_ROOT / doc_path
    if not full_doc_path.exists():
        message = f"{context} doc_path not found: {doc_path}"
        raise FileNotFoundError(message)
    summary = _require_non_empty_string(document, "summary", context=context)

    release_notes = document.get("release_notes")
    if not isinstance(release_notes, dict):
        message = f"{context}.release_notes must be a mapping"
        raise TypeError(message)

    release_notes_context = f"{context}.release_notes"
    intro = _require_non_empty_string(
        release_notes,
        "intro",
        context=release_notes_context,
    )
    expectations = _require_string_list(
        release_notes,
        "expectations",
        context=release_notes_context,
    )
    added = _require_string_list(
        release_notes,
        "added",
        context=release_notes_context,
    )
    changed = _require_string_list(
        release_notes,
        "changed",
        context=release_notes_context,
    )
    planned = _require_string_list(
        release_notes,
        "planned",
        context=release_notes_context,
    )

    sections = [
        f"# v{version} {title}",
        (
            f"Rendered from `docs/version/changelog/{channel}.yaml` for the "
            f"`{channel}` release channel. Human-readable changelog: `{doc_path}`."
        ),
        summary,
        "## Channel guidance\n\n" + intro,
        _render_section("Expectations", expectations),
        _render_section("Added", added),
        _render_section("Changed", changed),
        _render_section("Planned", planned),
    ]
    return "\n\n".join(sections)


def main() -> None:
    """Render release notes to stdout for shell consumption."""
    parser = argparse.ArgumentParser(
        description="Render repository-managed release notes for a release channel.",
    )
    parser.add_argument("--channel", required=True, choices=("stable", "beta", "alpha"))
    parser.add_argument("--version", required=True)
    sys.stdout.write(render_release_notes(**vars(parser.parse_args())))
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
