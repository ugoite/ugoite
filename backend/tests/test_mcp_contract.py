import pytest
from app.api.mcp import mcp

@pytest.mark.asyncio
async def test_mcp_tools_contract():
    tools = await mcp.list_tools()
    tool_names = [t.name for t in tools]
    assert "run_python_script" in tool_names
    
    tool = next(t for t in tools if t.name == "run_python_script")
    assert "code" in tool.inputSchema["properties"]
    assert "workspace_id" in tool.inputSchema["properties"]

@pytest.mark.asyncio
async def test_mcp_resources_contract():
    # Check resource templates
    templates = await mcp.list_resource_templates()
    template_uris = [t.uriTemplate for t in templates]
    
    assert "ieapp://{workspace_id}/notes/list" in template_uris
    assert "ieapp://{workspace_id}/notes/{note_id}" in template_uris
