"""REST API documentation consistency tests.

REQ-ENTRY-001: create-entry docs must use the implemented content payload field.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
API_REST_PATH = REPO_ROOT / "docs" / "spec" / "api" / "rest.md"
OPENAPI_PATH = REPO_ROOT / "docs" / "spec" / "api" / "openapi.yaml"


def _read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _require_mapping(value: Any, context: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise AssertionError(f"{context} must be a mapping")
    return value


def test_docs_req_entry_001_create_entry_payload_uses_content_field() -> None:
    """REQ-ENTRY-001: create-entry docs use the implemented `content` request field."""

    rest_text = _read_text(API_REST_PATH)
    details: list[str] = []

    rest_fragments = (
        '"content": "---\\nform: Entry\\n---\\n# My Entry\\n\\n## Body\\nValue"',
        "Create request bodies submit entry Markdown via the `content` field.",
    )
    details.extend(
        f"api/rest.md missing fragment: {fragment!r}"
        for fragment in rest_fragments
        if fragment not in rest_text
    )

    openapi = _require_mapping(yaml.safe_load(_read_text(OPENAPI_PATH)), "openapi root")
    paths = _require_mapping(openapi.get("paths"), "openapi paths")
    entry_post = _require_mapping(
        _require_mapping(paths.get("/spaces/{space_id}/entries"), "create-entry path").get("post"),
        "create-entry post operation",
    )
    request_body = _require_mapping(entry_post.get("requestBody"), "create-entry requestBody")
    content = _require_mapping(request_body.get("content"), "create-entry requestBody content")
    app_json = _require_mapping(content.get("application/json"), "create-entry application/json")
    schema = _require_mapping(app_json.get("schema"), "create-entry request schema")
    properties = _require_mapping(schema.get("properties"), "create-entry request properties")
    required = schema.get("required")
    example = _require_mapping(app_json.get("example"), "create-entry example")

    if not isinstance(required, list) or "content" not in required:
        details.append("api/openapi.yaml create-entry schema must require the content field")
    if "content" not in properties:
        details.append("api/openapi.yaml create-entry schema must define the content property")
    if "markdown" in properties:
        details.append("api/openapi.yaml create-entry schema must not advertise markdown")
    if "content" not in example:
        details.append("api/openapi.yaml create-entry example must use the content field")
    if "markdown" in example:
        details.append("api/openapi.yaml create-entry example must not use markdown")
    example_content = example.get("content")
    if not isinstance(example_content, str) or "form: Entry" not in example_content:
        details.append("api/openapi.yaml create-entry example must include form frontmatter")

    if details:
        raise AssertionError("; ".join(details))
