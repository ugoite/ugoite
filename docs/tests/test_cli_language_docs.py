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
    """REQ-OPS-014: ugoite-cli stays classified as Rust in docs and SBOM metadata."""
    cargo_text = _read_text(CLI_CARGO_PATH)
    index_text = _read_text(INDEX_PATH)
    stack_text = _read_text(STACK_PATH)
    security_text = _read_text(SECURITY_PATH)
    sbom_workflow = _read_text(SBOM_WORKFLOW_PATH)

    details = [
        f"{name} missing expected fragment: {fragment!r}"
        for name, text, fragment in (
            ("ugoite-cli/Cargo.toml", cargo_text, 'name = "ugoite-cli"'),
            (
                "docs/spec/index.md",
                index_text,
                "| `ugoite-cli` | Command-line interface for direct user "
                "interaction | Rust |",
            ),
            ("docs/spec/architecture/stack.md", stack_text, "### ugoite-cli (Rust)"),
            (
                "docs/spec/security/overview.md",
                security_text,
                "- Rust projects (`ugoite-core`, `ugoite-cli`)",
            ),
            (
                "docs/spec/security/overview.md",
                security_text,
                "- Python projects (`backend`)",
            ),
            (
                ".github/workflows/sbom-ci.yml",
                sbom_workflow,
                "ugoite-cli-rust.cdx.json",
            ),
        )
        if fragment not in text
    ]
    if "ugoite-cli-python.cdx.json" in sbom_workflow:
        details.append(
            ".github/workflows/sbom-ci.yml must not classify ugoite-cli as Python",
        )
    if details:
        raise AssertionError("; ".join(details))
