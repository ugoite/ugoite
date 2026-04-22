"""REQ-OPS-041: Browser walkthrough maps the main post-entry browser surfaces."""

from __future__ import annotations

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
BROWSER_FIRST_ENTRY_GUIDE_PATH = REPO_ROOT / "docs" / "guide" / "browser-first-entry.md"


def test_docs_req_ops_041_browser_walkthrough_maps_core_surfaces() -> None:
    """REQ-OPS-041: Browser walkthrough must explain the main browser surfaces."""
    guide_text = " ".join(
        BROWSER_FIRST_ENTRY_GUIDE_PATH.read_text(encoding="utf-8").split(),
    )
    required_fragments = (
        "**Dashboard**: the landing surface for quick-create actions",
        "**Entries**: the record list when you want to browse or reopen content",
        "**Forms**: the Form workspace for adding entry types",
        "**Search**: the derived search surface built from entries and Forms",
    )
    missing = [
        fragment for fragment in required_fragments if fragment not in guide_text
    ]
    if missing:
        message = (
            "browser-first-entry.md missing browser surface glossary guidance: "
            + ", ".join(missing)
        )
        raise AssertionError(message)
