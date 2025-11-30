import pytest
import os
import json
from pathlib import Path
from app.core.sandbox import run_in_sandbox
import ieapp


def test_run_python_script_integration(tmp_path):
    # 1. Setup Workspace
    root_path = tmp_path / "root"
    ieapp.create_workspace(root_path, "test-ws")
    ws_path = root_path / "workspaces" / "test-ws"

    # 2. Create Notes
    # Note: create_note expects content string
    ieapp.create_note(
        ws_path,
        "note-1",
        """
## Class
meeting
## Date
2025-01-01
""",
    )
    ieapp.create_note(
        ws_path,
        "note-2",
        """
## Class
task
## Status
pending
""",
    )

    # 3. Index Notes
    indexer = ieapp.Indexer(str(ws_path))
    indexer.run_once()

    # 4. Run Script
    # The script will query for 'meeting' and print the count.
    # And create a new note.
    script = """
import ieapp
import os

# Query
# Note: The indexer extracts headers. 'Class' header becomes 'Class' property.
meetings = ieapp.query(Class="meeting")
print(f"Found {len(meetings)} meetings")

# Create Note (using low-level API for now as we didn't wrap create_note)
ws_path = os.environ["IEAPP_WORKSPACE_ROOT"]
ieapp.create_note(ws_path, "note-3", "## Class\\nreport")
"""

    env = {"IEAPP_WORKSPACE_ROOT": str(ws_path)}
    result = run_in_sandbox(script, env=env)

    # 5. Verify
    assert result.returncode == 0, f"Script failed: {result.stderr}"
    assert "Found 1 meetings" in result.stdout

    # Verify note-3 created
    assert (ws_path / "notes" / "note-3" / "content.json").exists()
