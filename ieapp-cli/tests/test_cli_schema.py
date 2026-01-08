"""Tests for CLI schema commands."""

import json
from pathlib import Path

from typer.testing import CliRunner

from ieapp.cli import app

runner = CliRunner()


def test_cli_list_types() -> None:
    """Test listing column types (REQ-SCH-001)."""
    result = runner.invoke(app, ["schema", "list-types"])
    assert result.exit_code == 0
    assert "string" in result.stdout
    assert "date" in result.stdout


def test_cli_schema_update(tmp_path: Path) -> None:
    """Test updating schema via CLI (REQ-SCH-002)."""
    ws_root = tmp_path / "root"
    ws_root.mkdir()

    # 1. Create Workspace
    runner.invoke(app, ["create-workspace", str(ws_root), "test-ws"])

    ws_path = ws_root / "workspaces" / "test-ws"

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

    # 3. Define New Schema
    schema_file = tmp_path / "schema.json"
    schema_data = {
        "name": "Task",
        "fields": {
            "NewField": {"type": "string", "required": False},
        },
    }
    schema_file.write_text(json.dumps(schema_data))

    # 4. Run Update with Strategy
    strategies = json.dumps({"NewField": "DefaultVal", "Field": None})
    result = runner.invoke(
        app,
        [
            "schema",
            "update",
            str(ws_path),
            str(schema_file),
            "--strategies",
            strategies,
        ],
        catch_exceptions=False,
    )

    assert result.exit_code == 0
    assert "Migrated 1 note" in result.stdout

    # 5. Verify Note Content
    note_path = ws_path / "notes" / "note-1" / "content.json"
    content_json = json.loads(note_path.read_text())
    content = content_json["markdown"]

    # Verify "Field" is gone (strategy=None)
    assert "## Field" not in content
    # Verify "NewField" is present with value
    assert "## NewField" in content
    assert "DefaultVal" in content
