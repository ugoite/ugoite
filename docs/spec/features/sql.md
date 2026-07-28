---
title: 'Ugoite SQL'
---

Ugoite exposes two related surfaces:

1. **Saved SQL** under `/spaces/{space_id}/sql`, stored as versioned content in the Space.
2. **SQL sessions** under `/spaces/{space_id}/sql-sessions`, which store expiring query metadata and rerun queries to return count/rows.

The CLI `ugoite query <space> --sql ...` executes directly in core mode or creates/reads a remote SQL session in backend/API mode. SQL is parsed, planned, optimized, and executed by DataFusion over Iceberg Form providers. Each Form is exposed under its lowercase name with its Form fields plus the reserved system columns `_ugoite_id`, `_ugoite_title`, `_ugoite_created_at`, and `_ugoite_updated_at`; there is no cross-Form `entries` view or JSON-backed `links`/`assets` table.

Only one read-only DataFusion statement is accepted. The query context exposes only
authorized Form relations and explicitly allowlisted functions, then applies
Iceberg/DataFusion projection, predicate, and limit pushdown. Unsupported
relations, functions, and statement kinds fail without a compatibility fallback.

Parameters use DataFusion's native `$name` syntax and are bound as typed
DataFusion scalar values before planning. Ugoite never substitutes parameter
text into SQL.

SQL sessions do not become authoritative data. Their metadata is stored below `sql_sessions/`; result rows are regenerated from current Space tables.
