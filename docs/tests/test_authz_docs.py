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


def test_docs_req_sec_006_markdown_write_contract_covers_entry_mutations() -> None:
    """REQ-SEC-006: docs must name create, update, restore, and future import flows."""
    api_rest = _read_text(API_REST_PATH)
    for fragment in (
        "Create uses the submitted Markdown as the authorization target.",
        "Update MUST authorize both the current stored entry",
        "submitted Markdown\ntarget before writing.",
        "Restore MUST authorize both the current stored entry",
        "target revision\ncontent before writing the restored revision.",
        "Future import or bulk-migration\nadapters that submit Markdown MUST follow this same contract",
    ):
        assert fragment in api_rest


def test_docs_req_sec_006_markdown_write_contract_keeps_backend_thin() -> None:
    """REQ-SEC-006: docs must preserve backend orchestration and core ACL ownership."""
    interface_text = _read_text(INTERFACE_PATH)
    data_model = _read_text(DATA_MODEL_PATH)
    assert "backend MAY pre-check authorization" in interface_text
    assert "calling `ugoite-core` helpers" in interface_text
    assert "ACL evaluation still lives in `ugoite-core`" in interface_text
    assert "frontmatter-derived `form` ID is the canonical" in data_model
    assert "selector for Form ACL enforcement." in data_model
    assert "absence of a form falls back to the space-level" in data_model
    assert "`entry_write` policy." in data_model


def test_docs_req_sec_006_markdown_write_contract_defines_403_behavior() -> None:
    """REQ-SEC-006: docs and requirement metadata must define 403 authorization failures."""
    api_rest = _read_text(API_REST_PATH)
    requirements_text = _read_text(SECURITY_REQUIREMENTS_PATH)
    assert (
        "**Error**: `403 Forbidden` when space or form write authorization fails."
        in api_rest
    )
    assert (
        "Markdown-based entry writes MUST resolve the target Form from frontmatter"
        in requirements_text
    )
