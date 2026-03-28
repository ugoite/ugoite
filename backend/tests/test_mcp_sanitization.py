"""MCP sanitization tests.

REQ-API-003: MCP resource responses sanitize and label untrusted content.
"""

from app.mcp.sanitization import (
    MCP_ENTRY_CONTENT_NOTE,
    MCP_ENTRY_LIST_NOTE,
    MCP_ENTRY_LIST_TYPE,
    build_mcp_entry_list_response,
    sanitize_mcp_markdown,
)


def test_sanitize_mcp_markdown_req_api_003_preserves_code_and_autolinks() -> None:
    """REQ-API-003: sanitizer preserves literal Markdown code and autolinks."""
    markdown = (
        "Contact <user@example.com> and <https://example.com>.\n\n"
        "Inline `<T>` stays literal.\n\n"
        "```html\n"
        "<script>alert('keep literal')</script>\n"
        "<T>\n"
        "```\n"
        "<b>Hello</b> <img src=x onerror=alert('xss')>\n"
    )

    assert sanitize_mcp_markdown(markdown) == (
        "Contact <user@example.com> and <https://example.com>.\n\n"
        "Inline `<T>` stays literal.\n\n"
        "```html\n"
        "<script>alert('keep literal')</script>\n"
        "<T>\n"
        "```\n"
        "Hello \n"
    )


def test_sanitize_mcp_markdown_req_api_003_removes_scripts_and_comments() -> None:
    """REQ-API-003: sanitizer removes script blocks, comments, and raw tags."""
    markdown = (
        "Safe intro.\n"
        "<script >alert('drop')</script >\n"
        "<!-- hidden -->\n"
        "<div>Keep this text</div>\n"
    )

    sanitized = sanitize_mcp_markdown(markdown)

    assert "alert('drop')" not in sanitized
    assert "<!-- hidden -->" not in sanitized
    assert "<div>" not in sanitized
    assert "Safe intro." in sanitized
    assert "Keep this text" in sanitized


def test_build_mcp_entry_list_response_req_api_003_labels_sanitized_content() -> None:
    """REQ-API-003: MCP envelopes label content and sanitize entry fields."""
    response = build_mcp_entry_list_response(
        [
            {
                "id": "entry-1",
                "content": "<b>Hello</b>",
                "markdown": "<script>alert('drop')</script><i>World</i>",
            },
        ],
    )

    assert response == {
        "_type": MCP_ENTRY_LIST_TYPE,
        "_note": MCP_ENTRY_LIST_NOTE,
        "entries": [
            {
                "id": "entry-1",
                "content": "Hello",
                "markdown": "World",
                "_content_note": MCP_ENTRY_CONTENT_NOTE,
            },
        ],
    }
