"""Tests for CLI class commands."""

import json
from pathlib import Path

from typer.testing import CliRunner

from ieapp.classes import upsert_class
from ieapp.cli import app
from ieapp.notes import get_note

runner = CliRunner()


def test_cli_list_types() -> None:
    """Test listing column types (REQ-CLS-001)."""
    result = runner.invoke(app, ["class", "list-types"])
    assert result.exit_code == 0
    assert "string" in result.stdout
    assert "date" in result.stdout


def test_cli_class_update(tmp_path: Path) -> None:
    """Test updating class via CLI (REQ-CLS-002)."""
    ws_root = tmp_path / "root"
    ws_root.mkdir()

    # 1. Create Workspace
    runner.invoke(app, ["create-workspace", str(ws_root), "test-ws"])

    ws_path = ws_root / "workspaces" / "test-ws"

    upsert_class(
        str(ws_path),
        {
            "name": "Task",
            "template": "# Task\n\n## Field\n",
            "fields": {
                "Field": {"type": "string", "required": False},
            },
        },
    )

    # 2. Create Note
    res = runner.invoke(
        app,
        [
            "note",
            "create",
            str(ws_path),
            "note-1",
            "--content",
            "---\nclass: Task\n---\n## Field\nOldVal\n",
        ],
        catch_exceptions=False,
    )
    assert res.exit_code == 0

    # 3. Define New Class
    class_file = tmp_path / "class.json"
    class_data = {
        "name": "Task",
        "template": "# Task\n\n## NewField\n",
        "fields": {
            "NewField": {"type": "string", "required": False},
        },
    }
    class_file.write_text(json.dumps(class_data))

    # 4. Run Update with Strategy
    strategies = json.dumps({"NewField": "DefaultVal", "Field": None})
    result = runner.invoke(
        app,
        [
            "class",
            "update",
            str(ws_path),
            str(class_file),
            "--strategies",
            strategies,
        ],
        catch_exceptions=False,
    )

    assert result.exit_code == 0
    assert "Migrated 1 note" in result.stdout

    # 5. Verify Note Content
    note = get_note(str(ws_path), "note-1")
    content = note["content"]

    # Verify "Field" is gone (strategy=None)
    assert "## Field" not in content
    # Verify "NewField" is present with value
    assert "## NewField" in content
    assert "DefaultVal" in content
