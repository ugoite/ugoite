"""REQ-OPS-042: Browser walkthrough surfaces API and automation next steps."""

from __future__ import annotations

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
BROWSER_FIRST_ENTRY_GUIDE_PATH = REPO_ROOT / "docs" / "guide" / "browser-first-entry.md"


def test_docs_req_ops_042_browser_walkthrough_points_to_api_and_automation() -> None:
    """REQ-OPS-042: Browser walkthrough must expose API follow-up paths."""
    guide_text = " ".join(
        BROWSER_FIRST_ENTRY_GUIDE_PATH.read_text(encoding="utf-8").split(),
    )
    required_fragments = (
        "Need integrations or server-backed automation next?",
        "[REST API](../spec/api/rest.md)",
        "[Authentication Overview](auth-overview.md)",
        "browser, CLI, and API clients.",
    )
    missing = [
        fragment for fragment in required_fragments if fragment not in guide_text
    ]
    if missing:
        message = (
            "browser-first-entry.md missing API/automation next-step guidance: "
            + ", ".join(missing)
        )
        raise AssertionError(message)
