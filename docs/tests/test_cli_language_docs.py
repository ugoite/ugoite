"""CLI language consistency tests.

REQ-OPS-014: Top-level docs and SBOM metadata must classify ugoite-cli as Rust.
"""

from __future__ import annotations

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
INDEX_PATH = REPO_ROOT / "docs" / "spec" / "index.md"
STACK_PATH = REPO_ROOT / "docs" / "spec" / "architecture" / "stack.md"
SECURITY_PATH = REPO_ROOT / "docs" / "spec" / "security" / "overview.md"
SBOM_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "sbom-ci.yml"
CLI_CARGO_PATH = REPO_ROOT / "ugoite-cli" / "Cargo.toml"


def _read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def test_docs_req_ops_014_cli_language_is_consistent() -> None:
    """REQ-OPS-014: ugoite-cli must stay classified as Rust across docs and SBOM metadata."""
    cargo_text = _read_text(CLI_CARGO_PATH)
    index_text = _read_text(INDEX_PATH)
    stack_text = _read_text(STACK_PATH)
    security_text = _read_text(SECURITY_PATH)
    sbom_workflow = _read_text(SBOM_WORKFLOW_PATH)

    assert 'name = "ugoite-cli"' in cargo_text
    assert (
        "| `ugoite-cli` | Command-line interface for direct user interaction | Rust |"
        in index_text
    )
    assert "### ugoite-cli (Rust)" in stack_text
    assert "- Rust projects (`ugoite-core`, `ugoite-cli`)" in security_text
    assert "- Python projects (`backend`)" in security_text
    assert "ugoite-cli-rust.cdx.json" in sbom_workflow
    assert "ugoite-cli-python.cdx.json" not in sbom_workflow
