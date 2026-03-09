"""Authorization contract documentation tests.

REQ-SEC-006: Form ACL docs must define markdown write enforcement boundaries.
"""

from __future__ import annotations

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
API_REST_PATH = REPO_ROOT / "docs" / "spec" / "api" / "rest.md"
INTERFACE_PATH = (
    REPO_ROOT / "docs" / "spec" / "architecture" / "frontend-backend-interface.md"
)
DATA_MODEL_PATH = REPO_ROOT / "docs" / "spec" / "data-model" / "overview.md"
SECURITY_REQUIREMENTS_PATH = (
    REPO_ROOT / "docs" / "spec" / "requirements" / "security.yaml"
)


def _read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _normalize_whitespace(text: str) -> str:
    return " ".join(text.split())


def test_docs_req_sec_006_markdown_write_contract_covers_entry_mutations() -> None:
    """REQ-SEC-006: docs must name create, update, restore, and future import flows."""
    api_rest = _normalize_whitespace(_read_text(API_REST_PATH))
    details = [
        f"api/rest.md missing fragment: {fragment!r}"
        for fragment in (
            "Create uses the submitted Markdown as the authorization target.",
            "Update MUST authorize both the current stored entry",
            "submitted Markdown target before writing.",
            "Restore MUST authorize both the current stored entry",
            "target revision content before writing the restored revision.",
            "Future import or bulk-migration adapters that submit Markdown MUST "
            "follow this same contract",
        )
        if fragment not in api_rest
    ]
    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_sec_006_markdown_write_contract_keeps_backend_thin() -> None:
    """REQ-SEC-006: docs must preserve backend orchestration and core ACL ownership."""
    interface_text = _normalize_whitespace(_read_text(INTERFACE_PATH))
    data_model = _normalize_whitespace(_read_text(DATA_MODEL_PATH))
    details = [
        f"frontend-backend-interface.md missing fragment: {fragment!r}"
        for fragment in (
            "backend MAY pre-check authorization",
            "calling `ugoite-core` helpers",
            "ACL evaluation still lives in `ugoite-core`",
        )
        if fragment not in interface_text
    ]
    details.extend(
        [
            f"data-model/overview.md missing fragment: {fragment!r}"
            for fragment in (
                "frontmatter-derived `form` ID is the canonical",
                "selector for Form ACL enforcement.",
                "absence of a form falls back to the space-level",
                "`entry_write` policy.",
            )
            if fragment not in data_model
        ],
    )
    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_sec_006_markdown_write_contract_defines_403_behavior() -> None:
    """REQ-SEC-006: docs and requirement metadata define 403 auth failures."""
    api_rest = _normalize_whitespace(_read_text(API_REST_PATH))
    requirements_text = _normalize_whitespace(_read_text(SECURITY_REQUIREMENTS_PATH))
    details: list[str] = []
    if (
        "**Error**: `403 Forbidden` when space or form write authorization fails."
        not in api_rest
    ):
        details.append("api/rest.md must document 403 auth failures")
    if (
        "Markdown-based entry writes MUST resolve the target Form from frontmatter"
        not in requirements_text
    ):
        details.append(
            "requirements/security.yaml must define markdown-based form resolution",
        )
    if details:
        raise AssertionError("; ".join(details))
