"""MCP Contract Tests.

These tests verify that the MCP server implementation matches the specification
in docs/spec/04_api_and_mcp.md §2.

Contract tests ensure:
- All required resources are registered
- All required tools are registered
- Input schemas match expected parameters
- Resource URIs follow the spec format
"""

import pytest

from app.api.mcp import mcp


class TestMCPResourcesContract:
    """Test that MCP resources match the specification."""

    @pytest.mark.asyncio
    async def test_resource_templates_exist(self) -> None:
        """Verify all required resource templates are registered."""
        templates = await mcp.list_resource_templates()
        template_uris = [t.uriTemplate for t in templates]

        # Required resources per spec 04 §2 Resources
        required_resources = [
            "ieapp://{workspace_id}/notes/list",
            "ieapp://{workspace_id}/notes/{note_id}",
            "ieapp://{workspace_id}/notes/{note_id}/history",
            "ieapp://{workspace_id}/schema",
            "ieapp://{workspace_id}/links",
        ]

        for resource_uri in required_resources:
            assert resource_uri in template_uris, (
                f"Missing required resource: {resource_uri}"
            )

    @pytest.mark.asyncio
    async def test_notes_list_resource_template(self) -> None:
        """Verify notes list resource has correct URI template."""
        templates = await mcp.list_resource_templates()
        notes_list = next(
            (t for t in templates if "notes/list" in t.uriTemplate),
            None,
        )
        assert notes_list is not None
        assert "{workspace_id}" in notes_list.uriTemplate

    @pytest.mark.asyncio
    async def test_note_content_resource_template(self) -> None:
        """Verify single note resource has correct URI template."""
        templates = await mcp.list_resource_templates()
        note_content = next(
            (
                t
                for t in templates
                if "{note_id}" in t.uriTemplate and "history" not in t.uriTemplate
            ),
            None,
        )
        assert note_content is not None
        assert "{workspace_id}" in note_content.uriTemplate
        assert "{note_id}" in note_content.uriTemplate

    @pytest.mark.asyncio
    async def test_note_history_resource_template(self) -> None:
        """Verify note history resource has correct URI template."""
        templates = await mcp.list_resource_templates()
        history = next(
            (t for t in templates if "history" in t.uriTemplate),
            None,
        )
        assert history is not None
        assert "{workspace_id}" in history.uriTemplate
        assert "{note_id}" in history.uriTemplate


class TestMCPToolsContract:
    """Test that MCP tools match the specification."""

    @pytest.mark.asyncio
    async def test_required_tools_exist(self) -> None:
        """Verify all required tools are registered."""
        tools = await mcp.list_tools()
        tool_names = [t.name for t in tools]

        # Required tools per spec 04 §2 Tools
        required_tools = [
            "run_python_script",
            "search_notes",
            "notes_list",
            "notes_read",
            "notes_create",
            "notes_update",
            "notes_delete",
            "notes_restore",
        ]

        for tool_name in required_tools:
            assert tool_name in tool_names, f"Missing required tool: {tool_name}"

    @pytest.mark.asyncio
    async def test_run_python_script_schema(self) -> None:
        """Verify run_python_script has correct input schema."""
        tools = await mcp.list_tools()
        tool = next(t for t in tools if t.name == "run_python_script")

        props = tool.inputSchema["properties"]
        assert "code" in props, "run_python_script must have 'code' parameter"
        assert "workspace_id" in props, (
            "run_python_script must have 'workspace_id' parameter"
        )

    @pytest.mark.asyncio
    async def test_search_notes_schema(self) -> None:
        """Verify search_notes has correct input schema."""
        tools = await mcp.list_tools()
        tool = next(t for t in tools if t.name == "search_notes")

        props = tool.inputSchema["properties"]
        assert "query" in props, "search_notes must have 'query' parameter"
        assert "workspace_id" in props, (
            "search_notes must have 'workspace_id' parameter"
        )

    @pytest.mark.asyncio
    async def test_notes_list_schema(self) -> None:
        """Verify notes_list has correct input schema."""
        tools = await mcp.list_tools()
        tool = next(t for t in tools if t.name == "notes_list")

        props = tool.inputSchema["properties"]
        assert "workspace_id" in props

    @pytest.mark.asyncio
    async def test_notes_read_schema(self) -> None:
        """Verify notes_read has correct input schema."""
        tools = await mcp.list_tools()
        tool = next(t for t in tools if t.name == "notes_read")

        props = tool.inputSchema["properties"]
        assert "workspace_id" in props
        assert "note_id" in props

    @pytest.mark.asyncio
    async def test_notes_create_schema(self) -> None:
        """Verify notes_create has correct input schema."""
        tools = await mcp.list_tools()
        tool = next(t for t in tools if t.name == "notes_create")

        props = tool.inputSchema["properties"]
        assert "workspace_id" in props
        assert "title" in props
        assert "markdown" in props
        # Optional parameters are verified implicitly by schema

    @pytest.mark.asyncio
    async def test_notes_update_schema(self) -> None:
        """Verify notes_update has correct input schema per spec."""
        tools = await mcp.list_tools()
        tool = next(t for t in tools if t.name == "notes_update")

        props = tool.inputSchema["properties"]
        # Required per spec
        assert "workspace_id" in props
        assert "note_id" in props
        assert "parent_revision_id" in props, "Optimistic concurrency required"
        assert "markdown" in props

    @pytest.mark.asyncio
    async def test_notes_delete_schema(self) -> None:
        """Verify notes_delete has correct input schema."""
        tools = await mcp.list_tools()
        tool = next(t for t in tools if t.name == "notes_delete")

        props = tool.inputSchema["properties"]
        assert "workspace_id" in props
        assert "note_id" in props

    @pytest.mark.asyncio
    async def test_notes_restore_schema(self) -> None:
        """Verify notes_restore has correct input schema."""
        tools = await mcp.list_tools()
        tool = next(t for t in tools if t.name == "notes_restore")

        props = tool.inputSchema["properties"]
        assert "workspace_id" in props
        assert "note_id" in props
        assert "revision_id" in props


class TestMCPToolDescriptions:
    """Test that tools have proper descriptions."""

    @pytest.mark.asyncio
    async def test_tools_have_descriptions(self) -> None:
        """Verify all tools have descriptions."""
        tools = await mcp.list_tools()

        min_description_length = 10
        for tool in tools:
            assert tool.description, f"Tool {tool.name} must have a description"
            assert len(tool.description) > min_description_length, (
                f"Tool {tool.name} description too short"
            )
