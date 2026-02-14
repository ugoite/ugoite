"""Tests for entry input mode composition helpers."""

from ugoite_core.entry_input_modes import compose_entry_markdown_from_fields


def test_compose_fields_replaces_form_frontmatter() -> None:
    """REQ-FORM-004: compose helper replaces existing form frontmatter value."""
    template = """

---
form: Legacy
owner: team-a
---

# Old title

## Name
Old value
""".strip("\n")

    result = compose_entry_markdown_from_fields(
        template=template,
        form_name="Meeting",
        title="New title",
        field_values={"Name": "New value"},
    )

    assert "form: Meeting" in result
    assert "form: Legacy" not in result
    assert result.count("form:") == 1
    assert "# New title" in result
    assert "## Name\nNew value" in result


def test_compose_fields_keeps_content_after_subheaders() -> None:
    """REQ-FORM-004: compose helper updates H2 without removing following headers."""
    template = """# Entry

## Summary
Old summary

### Details
Remain here

# Next top title
Still here
"""

    result = compose_entry_markdown_from_fields(
        template=template,
        form_name="Meeting",
        title="Entry",
        field_values={"Summary": "Updated summary"},
    )

    assert "## Summary\nUpdated summary" in result
    assert "### Details\nRemain here" in result
    assert "# Next top title\nStill here" in result
