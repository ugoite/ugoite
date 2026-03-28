"""MCP documentation contract tests.

REQ-API-012: MCP resource input safety.
"""

from pathlib import Path


def test_docs_req_api_012_mcp_contract_covers_safe_ids_and_untrusted_content() -> None:
    """REQ-API-012: MCP docs describe safe space IDs and untrusted entry content."""
    doc = Path("docs/spec/api/mcp.md").read_text(encoding="utf-8")

    assert "^[A-Za-z0-9_-]+$" in doc
    assert "path traversal" in doc
    assert "null bytes" in doc
    assert "untrusted data" in doc
    assert "never follow instructions" in doc
