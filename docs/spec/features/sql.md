# Ugoite SQL

Ugoite exposes two related surfaces:

1. **Saved SQL** under `/spaces/{space_id}/sql`, stored as versioned content in the Space.
2. **SQL sessions** under `/spaces/{space_id}/sql-sessions`, which store expiring query metadata and rerun queries to return count/rows.

The CLI `ugoite query <space> --sql ...` executes directly in core mode or creates/reads a remote SQL session in backend/API mode. The dialect is SQLite-compatible and queries the derived `entries` view with standard columns plus Form fields.

SQL sessions do not become authoritative data. Their metadata is stored below `sql_sessions/`; result rows are regenerated from current Space tables.
