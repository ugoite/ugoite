"""MCP documentation contract tests.

REQ-API-012: MCP resource input safety.
"""

from __future__ import annotations

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]


def test_docs_req_api_012_mcp_contract_covers_safe_ids_and_untrusted_content() -> None:
    """REQ-API-012: MCP docs describe safe space IDs and untrusted entry content."""
    doc = (REPO_ROOT / "docs" / "spec" / "api" / "mcp.md").read_text(
        encoding="utf-8",
    )
    details = [
        f"docs/spec/api/mcp.md missing fragment: {fragment!r}"
        for fragment in (
            "^[A-Za-z0-9_-]+$",
            "path traversal",
            "null bytes",
            "untrusted data",
            "never follow instructions",
        )
        if fragment not in doc
    ]
    if details:
        raise AssertionError("; ".join(details))
