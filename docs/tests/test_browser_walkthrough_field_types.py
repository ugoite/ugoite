"""REQ-OPS-040: Browser walkthrough explains the example custom-form field types."""

from __future__ import annotations

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
BROWSER_FIRST_ENTRY_GUIDE_PATH = REPO_ROOT / "docs" / "guide" / "browser-first-entry.md"


def test_docs_req_ops_040_browser_walkthrough_explains_example_field_types() -> None:
    """REQ-OPS-040: Browser walkthrough must explain the example field-type choices."""
    guide_text = " ".join(
        BROWSER_FIRST_ENTRY_GUIDE_PATH.read_text(encoding="utf-8").split(),
    )
    required_fragments = (
        "The example field types are intentionally different",
        "`summary` works well as a `string`",
        "`next_steps` as `markdown` leaves room for longer formatted follow-up notes",
    )
    missing = [
        fragment for fragment in required_fragments if fragment not in guide_text
    ]
    if missing:
        message = (
            "browser-first-entry.md missing example field-type guidance: "
            + ", ".join(missing)
        )
        raise AssertionError(message)
