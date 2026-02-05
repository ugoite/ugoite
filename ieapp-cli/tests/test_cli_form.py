"""Tests for CLI form commands."""

import json
from pathlib import Path

from typer.testing import CliRunner

from ieapp.cli import app
from ieapp.entries import get_entry
from ieapp.forms import upsert_form

runner = CliRunner()


def test_cli_list_types() -> None:
    """Test listing column types (REQ-FORM-001)."""
    result = runner.invoke(app, ["form", "list-types"])
    assert result.exit_code == 0
    assert "string" in result.stdout
    assert "date" in result.stdout


def test_cli_form_update(tmp_path: Path) -> None:
    """Test updating form via CLI (REQ-FORM-002)."""
    ws_root = tmp_path / "root"
    ws_root.mkdir()

    # 1. Create Space
    runner.invoke(app, ["create-space", str(ws_root), "test-ws"])

    ws_path = ws_root / "spaces" / "test-ws"

    upsert_form(
        str(ws_path),
        {
            "name": "Task",
            "template": "# Task\n\n## Field\n",
            "fields": {
                "Field": {"type": "string", "required": False},
            },
        },
    )

    # 2. Create Entry
    res = runner.invoke(
        app,
        [
            "entry",
            "create",
            str(ws_path),
            "entry-1",
            "--content",
            "---\nform: Task\n---\n## Field\nOldVal\n",
        ],
        catch_exceptions=False,
    )
    assert res.exit_code == 0

    # 3. Define New Form
    form_file = tmp_path / "form.json"
    form_data = {
        "name": "Task",
        "template": "# Task\n\n## NewField\n",
        "fields": {
            "NewField": {"type": "string", "required": False},
        },
    }
    form_file.write_text(json.dumps(form_data))

    # 4. Run Update with Strategy
    strategies = json.dumps({"NewField": "DefaultVal", "Field": None})
    result = runner.invoke(
        app,
        [
            "form",
            "update",
            str(ws_path),
            str(form_file),
            "--strategies",
            strategies,
        ],
        catch_exceptions=False,
    )

    assert result.exit_code == 0
    assert "Migrated 1 entry" in result.stdout

    # 5. Verify Entry Content
    entry = get_entry(str(ws_path), "entry-1")
    content = entry["content"]

    # Verify "Field" is gone (strategy=None)
    assert "## Field" not in content
    # Verify "NewField" is present with value
    assert "## NewField" in content
    assert "DefaultVal" in content
