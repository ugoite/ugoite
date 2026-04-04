"""REST API documentation consistency tests.

REQ-ENTRY-001: create-entry docs must use the implemented content payload field.
"""

from __future__ import annotations

import json
from pathlib import Path

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
API_REST_PATH = REPO_ROOT / "docs" / "spec" / "api" / "rest.md"
OPENAPI_PATH = REPO_ROOT / "docs" / "spec" / "api" / "openapi.yaml"

YAMLMapping = dict[str, object]
EXPECTED_FORM_COLUMN_TYPES = [
    "string",
    "sql",
    "markdown",
    "number",
    "double",
    "float",
    "integer",
    "long",
    "boolean",
    "date",
    "time",
    "timestamp",
    "timestamp_tz",
    "timestamp_ns",
    "timestamp_tz_ns",
    "uuid",
    "row_reference",
    "binary",
    "list",
    "object_list",
]


def _read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _require_mapping(value: object, context: str) -> YAMLMapping:
    if not isinstance(value, dict):
        message = f"{context} must be a mapping"
        raise TypeError(message)
    return value


def _extract_json_code_block(section: str, context: str) -> object:
    if "```json" not in section:
        message = f"{context} must include a JSON code block"
        raise AssertionError(message)

    json_block = (
        section.split("```json", maxsplit=1)[1].split("```", maxsplit=1)[0].strip()
    )
    return json.loads(json_block)


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

    openapi = _require_mapping(
        yaml.safe_load(_read_text(OPENAPI_PATH)),
        "openapi root",
    )
    paths = _require_mapping(openapi.get("paths"), "openapi paths")
    entry_post = _require_mapping(
        _require_mapping(
            paths.get("/spaces/{space_id}/entries"),
            "create-entry path",
        ).get("post"),
        "create-entry post operation",
    )
    request_body = _require_mapping(
        entry_post.get("requestBody"),
        "create-entry requestBody",
    )
    content = _require_mapping(
        request_body.get("content"),
        "create-entry requestBody content",
    )
    app_json = _require_mapping(
        content.get("application/json"),
        "create-entry application/json",
    )
    schema = _require_mapping(app_json.get("schema"), "create-entry request schema")
    properties = _require_mapping(
        schema.get("properties"),
        "create-entry request properties",
    )
    required = schema.get("required")
    example = _require_mapping(app_json.get("example"), "create-entry example")

    if not isinstance(required, list) or "content" not in required:
        details.append(
            "api/openapi.yaml create-entry schema must require the content field",
        )
    if "content" not in properties:
        details.append(
            "api/openapi.yaml create-entry schema must define the content property",
        )
    if "markdown" in properties:
        details.append(
            "api/openapi.yaml create-entry schema must not advertise markdown",
        )
    if "content" not in example:
        details.append(
            "api/openapi.yaml create-entry example must use the content field",
        )
    if "markdown" in example:
        details.append(
            "api/openapi.yaml create-entry example must not use markdown",
        )
    example_content = example.get("content")
    if not isinstance(example_content, str) or "form: Entry" not in example_content:
        details.append(
            "api/openapi.yaml create-entry example must include form frontmatter",
        )

    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_form_001_list_column_types_response_contract() -> None:
    """REQ-FORM-001: list-column-types docs describe the full array response."""
    rest_text = _read_text(API_REST_PATH)
    details: list[str] = []

    list_column_types_section = rest_text.split("#### List Column Types", maxsplit=1)[1]
    list_column_types_section = list_column_types_section.split("\n---", maxsplit=1)[0]
    rest_fragments = ("**Response**: `200 OK`",)
    details.extend(
        f"api/rest.md list-column-types section missing fragment: {fragment!r}"
        for fragment in rest_fragments
        if fragment not in list_column_types_section
    )
    try:
        rest_example = _extract_json_code_block(
            list_column_types_section,
            "api/rest.md list-column-types section",
        )
    except (AssertionError, json.JSONDecodeError) as exc:
        details.append(str(exc))
    else:
        if rest_example != EXPECTED_FORM_COLUMN_TYPES:
            details.append(
                "api/rest.md list-column-types example must match the full implemented "
                "column type list",
            )

    openapi = _require_mapping(
        yaml.safe_load(_read_text(OPENAPI_PATH)),
        "openapi root",
    )
    paths = _require_mapping(openapi.get("paths"), "openapi paths")
    form_types_get = _require_mapping(
        _require_mapping(
            paths.get("/spaces/{space_id}/forms/types"),
            "list-column-types path",
        ).get("get"),
        "list-column-types get operation",
    )
    responses = _require_mapping(
        form_types_get.get("responses"),
        "list-column-types responses",
    )
    ok_response = _require_mapping(
        responses.get("200"),
        "list-column-types 200 response",
    )
    content = _require_mapping(ok_response.get("content"), "list-column-types content")
    app_json = _require_mapping(
        content.get("application/json"),
        "list-column-types application/json response",
    )
    schema = _require_mapping(app_json.get("schema"), "list-column-types schema")
    items = _require_mapping(schema.get("items"), "list-column-types schema items")
    example = app_json.get("example")

    if schema.get("type") != "array":
        details.append("api/openapi.yaml list-column-types response must be an array")
    if items.get("type") != "string":
        details.append(
            "api/openapi.yaml list-column-types array items must be strings",
        )
    if items.get("enum") != EXPECTED_FORM_COLUMN_TYPES:
        details.append(
            "api/openapi.yaml list-column-types item enum must match the full "
            "implemented column type list",
        )
    if example != EXPECTED_FORM_COLUMN_TYPES:
        details.append(
            "api/openapi.yaml list-column-types example must match the full "
            "implemented column type list",
        )

    if details:
        raise AssertionError("; ".join(details))
