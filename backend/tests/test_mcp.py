"""Tests for MCP server implementation.

TDD Step 2: run_python_script can import ieapp, query data, update note
TDD Step 3: Contract test for MCP resource serialization
"""

import json
from pathlib import Path

from fastapi.testclient import TestClient


class TestMCPResources:
    """Test MCP resource endpoints."""

    def test_notes_list_resource(
        self,
        test_client: TestClient,
        temp_workspace_root: Path,  # noqa: ARG002
    ) -> None:
        """Test ieapp://{workspace_id}/notes/list resource."""
        # Create workspace and notes first
        test_client.post("/workspaces", json={"name": "mcp-test"})
        test_client.post(
            "/workspaces/mcp-test/notes",
            json={"content": "# Test Note\n\n## Status\nopen"},
        )

        # Access MCP resource via tool call
        # Note: In production, this would be via MCP protocol
        # For testing, we verify the underlying functionality
        from app.mcp import get_notes_list

        result = get_notes_list("mcp-test")
        data = json.loads(result)

        assert isinstance(data, list)
        assert len(data) >= 1
        assert "id" in data[0]

    def test_note_content_resource(
        self,
        test_client: TestClient,
        temp_workspace_root: Path,  # noqa: ARG002
    ) -> None:
        """Test ieapp://{workspace_id}/notes/{note_id} resource."""
        # Create workspace and note
        test_client.post("/workspaces", json={"name": "mcp-test"})
        response = test_client.post(
            "/workspaces/mcp-test/notes",
            json={"content": "# My Note\n\n## Description\nTest content"},
        )
        note_id = response.json()["id"]

        from app.mcp import get_note_content

        result = get_note_content("mcp-test", note_id)
        data = json.loads(result)

        assert "markdown" in data
        assert "# My Note" in data["markdown"]

    def test_resource_error_handling(
        self,
        test_client: TestClient,
        temp_workspace_root: Path,  # noqa: ARG002
    ) -> None:
        """Test that resources handle errors gracefully."""
        from app.mcp import get_note_content, get_notes_list

        # Non-existent workspace
        result = get_notes_list("nonexistent")
        data = json.loads(result)
        assert "error" in data

        # Non-existent note
        test_client.post("/workspaces", json={"name": "mcp-test"})
        result = get_note_content("mcp-test", "nonexistent-note")
        data = json.loads(result)
        assert "error" in data


class TestMCPTools:
    """Test MCP tool endpoints."""

    def test_notes_list_tool(
        self,
        test_client: TestClient,
        temp_workspace_root: Path,  # noqa: ARG002
    ) -> None:
        """Test notes_list tool."""
        test_client.post("/workspaces", json={"name": "mcp-test"})
        test_client.post(
            "/workspaces/mcp-test/notes",
            json={"content": "# Note 1"},
        )

        from app.mcp import notes_list

        result = notes_list("mcp-test")

        assert "notes" in result
        assert "count" in result
        assert result["count"] >= 1

    def test_notes_create_tool(
        self,
        test_client: TestClient,
        temp_workspace_root: Path,  # noqa: ARG002
    ) -> None:
        """Test notes_create tool."""
        test_client.post("/workspaces", json={"name": "mcp-test"})

        from app.mcp import notes_create

        result = notes_create(
            workspace_id="mcp-test",
            title="Created via MCP",
            markdown="This is a test note created via MCP.",
            note_class="task",
            tags=["test", "mcp"],
        )

        assert "id" in result
        assert result["status"] == "created"

    def test_notes_read_tool(
        self,
        test_client: TestClient,
        temp_workspace_root: Path,  # noqa: ARG002
    ) -> None:
        """Test notes_read tool."""
        test_client.post("/workspaces", json={"name": "mcp-test"})
        response = test_client.post(
            "/workspaces/mcp-test/notes",
            json={"content": "# Read Test\n\nContent here"},
        )
        note_id = response.json()["id"]

        from app.mcp import notes_read

        result = notes_read("mcp-test", note_id)

        assert "markdown" in result
        assert "# Read Test" in result["markdown"]

    def test_notes_update_tool(
        self,
        test_client: TestClient,
        temp_workspace_root: Path,  # noqa: ARG002
    ) -> None:
        """Test notes_update tool with optimistic concurrency."""
        test_client.post("/workspaces", json={"name": "mcp-test"})
        response = test_client.post(
            "/workspaces/mcp-test/notes",
            json={"content": "# Original"},
        )
        note_id = response.json()["id"]

        # Get current revision
        note_response = test_client.get(f"/workspaces/mcp-test/notes/{note_id}")
        current_revision = note_response.json()["revision_id"]

        from app.mcp import notes_update

        result = notes_update(
            workspace_id="mcp-test",
            note_id=note_id,
            parent_revision_id=current_revision,
            markdown="# Updated\n\nNew content",
        )

        assert "revision_id" in result
        assert result["revision_id"] != current_revision

    def test_notes_update_conflict(
        self,
        test_client: TestClient,
        temp_workspace_root: Path,  # noqa: ARG002
    ) -> None:
        """Test notes_update tool returns conflict on revision mismatch."""
        test_client.post("/workspaces", json={"name": "mcp-test"})
        response = test_client.post(
            "/workspaces/mcp-test/notes",
            json={"content": "# Original"},
        )
        note_id = response.json()["id"]

        from app.mcp import notes_update

        # Use wrong revision ID
        result = notes_update(
            workspace_id="mcp-test",
            note_id=note_id,
            parent_revision_id="wrong-revision-id",
            markdown="# Updated",
        )

        assert "error" in result
        assert "Conflict" in result["error"] or "revision" in result["error"].lower()

    def test_notes_delete_tool(
        self,
        test_client: TestClient,
        temp_workspace_root: Path,  # noqa: ARG002
    ) -> None:
        """Test notes_delete tool."""
        test_client.post("/workspaces", json={"name": "mcp-test"})
        response = test_client.post(
            "/workspaces/mcp-test/notes",
            json={"content": "# To Delete"},
        )
        note_id = response.json()["id"]

        from app.mcp import notes_delete

        result = notes_delete("mcp-test", note_id)

        assert result["status"] == "deleted"


class TestRunPythonScriptTool:
    """Test run_python_script MCP tool (TDD Step 2)."""

    def test_basic_script_execution(
        self,
        test_client: TestClient,
        temp_workspace_root: Path,  # noqa: ARG002
    ) -> None:
        """Test basic Python script execution."""
        test_client.post("/workspaces", json={"name": "mcp-test"})

        from app.mcp import run_python_script_tool

        result = run_python_script_tool(
            code='print("Hello from sandbox")',
            workspace_id="mcp-test",
        )

        assert result["success"]
        assert "Hello from sandbox" in result["stdout"]

    def test_script_with_json_output(
        self,
        test_client: TestClient,
        temp_workspace_root: Path,  # noqa: ARG002
    ) -> None:
        """Test script producing JSON output."""
        test_client.post("/workspaces", json={"name": "mcp-test"})

        from app.mcp import run_python_script_tool

        result = run_python_script_tool(
            code="""
import json
data = {"count": 42, "items": ["a", "b", "c"]}
print(json.dumps(data))
""",
            workspace_id="mcp-test",
        )

        assert result["success"]
        output = json.loads(result["stdout"].strip())
        assert output["count"] == 42

    def test_script_security_violation(
        self,
        test_client: TestClient,
        temp_workspace_root: Path,  # noqa: ARG002
    ) -> None:
        """Test that security violations are caught."""
        test_client.post("/workspaces", json={"name": "mcp-test"})

        from app.mcp import run_python_script_tool

        result = run_python_script_tool(
            code='import os\nos.system("ls")',
            workspace_id="mcp-test",
        )

        assert not result["success"]
        assert result["error_type"] == "SecurityError"

    def test_script_error_handling(
        self,
        test_client: TestClient,
        temp_workspace_root: Path,  # noqa: ARG002
    ) -> None:
        """Test that script errors are handled gracefully."""
        test_client.post("/workspaces", json={"name": "mcp-test"})

        from app.mcp import run_python_script_tool

        result = run_python_script_tool(
            code='raise ValueError("Test error")',
            workspace_id="mcp-test",
        )

        assert not result["success"]
        assert result["error_type"] == "ValueError"
        assert "Test error" in result["error_message"]


class TestMCPContractSerialization:
    """Contract tests for MCP resource serialization (TDD Step 3).

    These tests verify that MCP responses match the expected JSON schema
    defined in the spec.
    """

    def test_notes_list_schema(
        self,
        test_client: TestClient,
        temp_workspace_root: Path,  # noqa: ARG002
    ) -> None:
        """Test that notes list matches expected schema."""
        test_client.post("/workspaces", json={"name": "mcp-test"})
        test_client.post(
            "/workspaces/mcp-test/notes",
            json={"content": "# Test\n\n## Status\nopen"},
        )

        from app.mcp import notes_list

        result = notes_list("mcp-test")

        # Verify schema
        assert "notes" in result
        assert "count" in result
        assert isinstance(result["notes"], list)
        assert isinstance(result["count"], int)

        if result["notes"]:
            note = result["notes"][0]
            # Required fields per spec
            assert "id" in note
            assert isinstance(note["id"], str)

    def test_note_read_schema(
        self,
        test_client: TestClient,
        temp_workspace_root: Path,  # noqa: ARG002
    ) -> None:
        """Test that note read matches expected schema."""
        test_client.post("/workspaces", json={"name": "mcp-test"})
        response = test_client.post(
            "/workspaces/mcp-test/notes",
            json={"content": "# Test Note\n\n## Date\n2025-01-01"},
        )
        note_id = response.json()["id"]

        from app.mcp import notes_read

        result = notes_read("mcp-test", note_id)

        # Verify schema per spec
        assert "markdown" in result
        assert "revision_id" in result
        assert "frontmatter" in result
        assert "sections" in result

        assert isinstance(result["markdown"], str)
        assert isinstance(result["revision_id"], str)

    def test_run_python_script_result_schema(
        self,
        test_client: TestClient,
        temp_workspace_root: Path,  # noqa: ARG002
    ) -> None:
        """Test that run_python_script result matches expected schema."""
        test_client.post("/workspaces", json={"name": "mcp-test"})

        from app.mcp import run_python_script_tool

        result = run_python_script_tool(
            code='print("test")',
            workspace_id="mcp-test",
        )

        # Required fields per SandboxResult spec
        assert "stdout" in result
        assert "stderr" in result
        assert "success" in result

        assert isinstance(result["stdout"], str)
        assert isinstance(result["stderr"], str)
        assert isinstance(result["success"], bool)

    def test_error_response_schema(
        self,
        test_client: TestClient,
        temp_workspace_root: Path,  # noqa: ARG002
    ) -> None:
        """Test that error responses have consistent schema."""
        from app.mcp import notes_list, notes_read

        # Non-existent workspace
        result = notes_list("nonexistent")
        assert "error" in result
        assert isinstance(result["error"], str)

        # Non-existent note
        test_client.post("/workspaces", json={"name": "mcp-test"})
        result = notes_read("mcp-test", "nonexistent")
        assert "error" in result
        assert isinstance(result["error"], str)
