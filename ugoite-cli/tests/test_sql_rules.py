"""Tests for shared SQL rules."""

import json

from typer.testing import CliRunner

from ugoite.cli import app

runner = CliRunner()


def test_cli_sql_lint_reports_errors() -> None:
    """REQ-SRCH-003: SQL lint surfaces missing SELECT."""
    result = runner.invoke(app, ["sql", "lint", "FROM entries"])
    assert result.exit_code == 1
    assert "SELECT" in result.stdout


def test_cli_sql_complete_suggests_tables() -> None:
    """REQ-SRCH-003: SQL completion suggests tables."""
    result = runner.invoke(app, ["sql", "complete", "SELECT * FROM "])
    assert result.exit_code == 0
    suggestions = json.loads(result.stdout)
    assert "entries" in suggestions
