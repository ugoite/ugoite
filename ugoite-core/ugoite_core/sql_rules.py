"""Shared Ugoite SQL linting and completion rules."""

from __future__ import annotations

import json
import os
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any

DEFAULT_RULES_PATH = (
    Path(__file__).resolve().parents[2] / "shared" / "sql" / "ugoite-sql-rules.json"
)


@dataclass(frozen=True)
class SqlLintDiagnostic:
    """Represents a SQL lint issue."""

    message: str
    severity: str = "error"
    start: int | None = None
    end: int | None = None


def _resolve_rules_path(path: str | None = None) -> Path:
    env_path = os.getenv("UGOITE_SQL_RULES_PATH")
    if path:
        return Path(path)
    if env_path:
        return Path(env_path)
    return DEFAULT_RULES_PATH


def load_sql_rules(path: str | None = None) -> dict[str, Any]:
    """Load SQL rules from the shared config file."""
    rules_path = _resolve_rules_path(path)
    if not rules_path.exists():
        msg = f"SQL rules file not found: {rules_path}"
        raise FileNotFoundError(msg)
    data = json.loads(rules_path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        msg = "SQL rules must be a JSON object"
        raise TypeError(msg)
    return data


def build_sql_schema(
    forms: list[dict[str, Any]],
    rules: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Return SQL completion schema based on shared rules and forms."""
    rules = rules or load_sql_rules()
    base_columns = list(rules.get("base_columns", []))
    base_tables = list(rules.get("base_tables", ["entries"]))
    table_columns = rules.get("table_columns", {})

    form_field_set: set[str] = set()
    for item in forms:
        fields = item.get("fields") or {}
        if isinstance(fields, dict):
            form_field_set.update(fields.keys())

    union_fields = base_columns + sorted(form_field_set)

    tables: dict[str, list[str]] = {}
    for table in base_tables:
        columns = None
        if isinstance(table_columns, dict):
            columns = table_columns.get(table)
        if isinstance(columns, list):
            tables[table] = list(columns)
        else:
            tables[table] = union_fields

    for item in forms:
        name = item.get("name")
        if not name:
            continue
        fields = item.get("fields") or {}
        if isinstance(fields, dict):
            tables[str(name)] = base_columns + sorted(fields.keys())

    return {
        "tables": tables,
        "keywords": list(rules.get("keywords", [])),
    }


def lint_sql(
    query: str,
    rules: dict[str, Any] | None = None,
) -> list[SqlLintDiagnostic]:
    """Return lint diagnostics for a Ugoite SQL query."""
    rules = rules or load_sql_rules()
    lint_rules = rules.get("lint", {}) if isinstance(rules.get("lint"), dict) else {}

    diagnostics: list[SqlLintDiagnostic] = []
    leading_whitespace = len(query) - len(query.lstrip())
    trimmed = query.strip()

    if not trimmed:
        diagnostics.append(
            SqlLintDiagnostic(
                message="Query is required",
                severity="error",
                start=0,
                end=min(len(query), 1),
            ),
        )
        return diagnostics

    if lint_rules.get("require_select", True) and not re.search(
        r"\bselect\b",
        trimmed,
        re.IGNORECASE,
    ):
        diagnostics.append(
            SqlLintDiagnostic(
                message="Query must start with SELECT",
                severity="error",
                start=leading_whitespace,
                end=leading_whitespace + min(len(trimmed), 6),
            ),
        )

    if lint_rules.get("require_from", True) and not re.search(
        r"\bfrom\b",
        trimmed,
        re.IGNORECASE,
    ):
        diagnostics.append(
            SqlLintDiagnostic(
                message="Query must include FROM",
                severity="error",
                start=max(0, len(query) - 1),
                end=len(query),
            ),
        )

    if lint_rules.get("single_statement_only", True):
        semicolon_index = query.find(";")
        if semicolon_index != -1 and semicolon_index < len(query) - 1:
            diagnostics.append(
                SqlLintDiagnostic(
                    message="Only a single statement is supported",
                    severity="warning",
                    start=semicolon_index,
                    end=semicolon_index + 1,
                ),
            )

    if lint_rules.get("limit_requires_number", True):
        limit_match = re.search(r"\blimit\b\s+([^\s;]+)", query, re.IGNORECASE)
        if limit_match and limit_match.group(1):
            value = limit_match.group(1)
            if not value.isdigit():
                start = limit_match.start(1)
                diagnostics.append(
                    SqlLintDiagnostic(
                        message="LIMIT value must be a number",
                        severity="error",
                        start=start,
                        end=start + len(value),
                    ),
                )

    return diagnostics


def sql_completions(
    query: str,
    forms: list[dict[str, Any]],
    rules: dict[str, Any] | None = None,
) -> list[str]:
    """Return completion suggestions based on query context."""
    schema = build_sql_schema(forms, rules)
    tables = sorted(schema.get("tables", {}).keys())
    keywords = sorted(schema.get("keywords", []))

    table_match = re.search(r"\bfrom\b\s+([A-Za-z_][\w]*)", query, re.IGNORECASE)
    table_name = table_match.group(1) if table_match else None

    columns = []
    if table_name and table_name in schema.get("tables", {}):
        columns = sorted(schema["tables"][table_name])
    elif "entries" in schema.get("tables", {}):
        columns = sorted(schema["tables"]["entries"])

    if re.search(r"\bfrom\b\s*$", query, re.IGNORECASE):
        return tables

    if re.search(r"\b(where|and|or|order\s+by)\b\s*$", query, re.IGNORECASE):
        return columns

    return sorted(set(tables + columns + keywords))
