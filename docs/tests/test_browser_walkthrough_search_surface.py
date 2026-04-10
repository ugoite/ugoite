"""REQ-OPS-038: Browser walkthrough explains the Search surface to newcomers."""

from __future__ import annotations

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
BROWSER_FIRST_ENTRY_GUIDE_PATH = REPO_ROOT / "docs" / "guide" / "browser-first-entry.md"


def test_docs_req_ops_038_browser_walkthrough_explains_search_surface() -> None:
    """REQ-OPS-038: Browser walkthrough must explain Search and why it matters."""
    guide_text = BROWSER_FIRST_ENTRY_GUIDE_PATH.read_text(encoding="utf-8")
    required_fragments = (
        "derived search surface built from entries and Forms",
        "discoverable beyond a single page",
    )
    missing = [
        fragment for fragment in required_fragments if fragment not in guide_text
    ]
    if missing:
        message = (
            "browser-first-entry.md missing Search walkthrough guidance: "
            + ", ".join(
                missing,
            )
        )
        raise AssertionError(message)
