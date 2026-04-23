"""REQ-E2E-008: Browser walkthrough recaps a successful first entry."""

from __future__ import annotations

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
BROWSER_FIRST_ENTRY_GUIDE_PATH = REPO_ROOT / "docs" / "guide" / "browser-first-entry.md"


def test_docs_req_e2e_008_browser_walkthrough_recap_after_first_entry() -> None:
    """REQ-E2E-008: Browser walkthrough must add a first-entry recap."""
    guide_text = " ".join(
        BROWSER_FIRST_ENTRY_GUIDE_PATH.read_text(encoding="utf-8").split(),
    )
    required_fragments = (
        "confidence checks that prove the first-run path worked",
        "the entry detail page is open",
        "the new record can be reopened from",
        "the derived **Search** surface can discover",
    )
    missing = [
        fragment for fragment in required_fragments if fragment not in guide_text
    ]
    if missing:
        message = (
            "browser-first-entry.md missing post-entry recap guidance: "
            + ", ".join(missing)
        )
        raise AssertionError(message)
