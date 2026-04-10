"""REQ-OPS-037: Browser walkthrough docs keep Form terminology."""

from __future__ import annotations

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
BROWSER_FIRST_ENTRY_GUIDE_PATH = REPO_ROOT / "docs" / "guide" / "browser-first-entry.md"


def test_docs_req_ops_037_browser_walkthrough_uses_form_terminology() -> None:
    """REQ-OPS-037: Browser walkthrough must keep the starter flow on Form wording."""
    guide_text = BROWSER_FIRST_ENTRY_GUIDE_PATH.read_text(encoding="utf-8").lower()
    if "schema" in guide_text:
        message = (
            "browser-first-entry.md must keep the starter entry flow on user-facing "
            "Form terminology instead of reintroducing legacy schema wording"
        )
        raise AssertionError(message)
