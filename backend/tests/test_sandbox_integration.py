"""Integration tests for the sandbox with ieapp library.

These tests verify that the sandbox can correctly execute code that uses
the ieapp library to manipulate workspaces and notes.

Spec Reference: 02 Story 1 & 8, 04 §2 run_python_script
"""

from pathlib import Path

import ieapp

from app.core.sandbox import SandboxErrorType, run_in_sandbox


class TestSandboxIeappIntegration:
    """Integration tests for ieapp operations in the sandbox."""

    def test_run_python_script_query_notes(self, tmp_path: Path) -> None:
        """Test that scripts can query notes via ieapp.query()."""
        # 1. Setup Workspace
        root_path = tmp_path / "root"
        ieapp.create_workspace(root_path, "test-ws")
        ws_path = root_path / "workspaces" / "test-ws"

        # 2. Create Notes with H2 headers for indexing
        ieapp.create_note(
            ws_path,
            "note-1",
            """# Meeting Note
## Class
meeting
## Date
2025-01-01
""",
        )
        ieapp.create_note(
            ws_path,
            "note-2",
            """# Task Note
## Class
task
## Status
pending
""",
        )

        # 3. Index Notes
        indexer = ieapp.Indexer(str(ws_path))
        indexer.run_once()

        # 4. Run Script that queries for meetings
        script = """
import ieapp

# Query for notes with Class="meeting"
meetings = ieapp.query(Class="meeting")
print(f"Found {len(meetings)} meetings")
for m in meetings:
    print(f"- {m.get('id', 'unknown')}")
"""
        env = {"IEAPP_WORKSPACE_ROOT": str(ws_path)}
        result = run_in_sandbox(script, env=env)

        # 5. Verify
        assert result.returncode == 0, f"Script failed: {result.stderr}"
        assert "Found 1 meetings" in result.stdout
        assert "note-1" in result.stdout

    def test_run_python_script_create_note(self, tmp_path: Path) -> None:
        """Test that scripts can create new notes."""
        # Setup
        root_path = tmp_path / "root"
        ieapp.create_workspace(root_path, "test-ws")
        ws_path = root_path / "workspaces" / "test-ws"

        # Run script that creates a note
        script = """
import ieapp
import os

ws_path = os.environ["IEAPP_WORKSPACE_ROOT"]
ieapp.create_note(ws_path, "new-note", "# New Note\\n\\nCreated by script")
print("Note created successfully")
"""
        env = {"IEAPP_WORKSPACE_ROOT": str(ws_path)}
        result = run_in_sandbox(script, env=env)

        # Verify
        assert result.returncode == 0, f"Script failed: {result.stderr}"
        assert "Note created successfully" in result.stdout

        # Verify note was actually created
        assert (ws_path / "notes" / "new-note" / "content.json").exists()

    def test_run_python_script_list_notes(self, tmp_path: Path) -> None:
        """Test that scripts can list all notes."""
        # Setup
        root_path = tmp_path / "root"
        ieapp.create_workspace(root_path, "test-ws")
        ws_path = root_path / "workspaces" / "test-ws"

        # Create some notes
        ieapp.create_note(ws_path, "note-a", "# Note A")
        ieapp.create_note(ws_path, "note-b", "# Note B")

        # Run script
        script = """
import ieapp
import os

ws_path = os.environ["IEAPP_WORKSPACE_ROOT"]
notes = ieapp.list_notes(ws_path)
print(f"Found {len(notes)} notes")
for note in notes:
    print(f"- {note['id']}: {note.get('title', 'Untitled')}")
"""
        env = {"IEAPP_WORKSPACE_ROOT": str(ws_path)}
        result = run_in_sandbox(script, env=env)

        # Verify
        assert result.returncode == 0, f"Script failed: {result.stderr}"
        assert "Found 2 notes" in result.stdout
        assert "note-a" in result.stdout
        assert "note-b" in result.stdout

    def test_run_python_script_update_note(self, tmp_path: Path) -> None:
        """Test that scripts can update existing notes."""
        # Setup
        root_path = tmp_path / "root"
        ieapp.create_workspace(root_path, "test-ws")
        ws_path = root_path / "workspaces" / "test-ws"

        # Create initial note
        ieapp.create_note(ws_path, "update-me", "# Original Title")

        # Get the revision ID
        note = ieapp.get_note(str(ws_path), "update-me")
        revision_id = note["revision_id"]

        # Run script that updates the note
        script = f"""
import ieapp
import os

ws_path = os.environ["IEAPP_WORKSPACE_ROOT"]
ieapp.update_note(
    ws_path,
    "update-me",
    "# Updated Title\\n\\nNew content here",
    "{revision_id}"
)
print("Note updated successfully")
"""
        env = {"IEAPP_WORKSPACE_ROOT": str(ws_path)}
        result = run_in_sandbox(script, env=env)

        # Verify
        assert result.returncode == 0, f"Script failed: {result.stderr}"
        assert "Note updated successfully" in result.stdout

        # Verify note was updated
        updated_note = ieapp.get_note(str(ws_path), "update-me")
        assert "Updated Title" in updated_note["markdown"]

    def test_run_python_script_error_handling(self, tmp_path: Path) -> None:
        """Test that script errors are properly reported."""
        root_path = tmp_path / "root"
        ieapp.create_workspace(root_path, "test-ws")
        ws_path = root_path / "workspaces" / "test-ws"

        # Script with intentional error
        script = """
import ieapp
import os

ws_path = os.environ["IEAPP_WORKSPACE_ROOT"]
# Try to get a non-existent note
note = ieapp.get_note(ws_path, "non-existent-note")
"""
        env = {"IEAPP_WORKSPACE_ROOT": str(ws_path)}
        result = run_in_sandbox(script, env=env)

        # Should fail with appropriate error
        assert result.returncode != 0
        assert (
            "FileNotFoundError" in result.stderr or "not found" in result.stderr.lower()
        )


class TestSandboxAgenticWorkflow:
    """Test agentic refactoring workflow (Story 8)."""

    def test_batch_processing_workflow(self, tmp_path: Path) -> None:
        """Test a batch processing script like an agent would use."""
        # Setup workspace with multiple notes
        root_path = tmp_path / "root"
        ieapp.create_workspace(root_path, "test-ws")
        ws_path = root_path / "workspaces" / "test-ws"

        # Create notes to be processed
        for i in range(3):
            ieapp.create_note(
                ws_path,
                f"task-{i}",
                f"""# Task {i}
## Class
task
## Status
pending
""",
            )

        # Index
        indexer = ieapp.Indexer(str(ws_path))
        indexer.run_once()

        # Run batch processing script
        script = """
import ieapp
import os

ws_path = os.environ["IEAPP_WORKSPACE_ROOT"]

# Find all pending tasks
tasks = ieapp.query(Class="task", Status="pending")
print(f"Processing {len(tasks)} pending tasks")

# Report on them
for task in tasks:
    note = ieapp.get_note(ws_path, task["id"])
    title = task.get("title", "Untitled")
    print(f"- {task['id']}: {title}")

print("Batch processing complete")
"""
        env = {"IEAPP_WORKSPACE_ROOT": str(ws_path)}
        result = run_in_sandbox(script, env=env)

        assert result.returncode == 0, f"Script failed: {result.stderr}"
        assert "Processing 3 pending tasks" in result.stdout
        assert "Batch processing complete" in result.stdout


class TestSandboxTimeout:
    """Test timeout handling in integration scenarios."""

    def test_long_running_script_timeout(self, tmp_path: Path) -> None:
        """Test that long-running scripts are properly terminated."""
        root_path = tmp_path / "root"
        ieapp.create_workspace(root_path, "test-ws")
        ws_path = root_path / "workspaces" / "test-ws"

        # Script that runs too long
        script = """
import time
print("Starting long operation...")
time.sleep(60)
print("This should not be printed")
"""
        env = {"IEAPP_WORKSPACE_ROOT": str(ws_path)}
        result = run_in_sandbox(script, env=env, timeout=2)

        assert result.error_type == SandboxErrorType.TIMEOUT
        assert result.returncode != 0
        assert "This should not be printed" not in result.stdout
