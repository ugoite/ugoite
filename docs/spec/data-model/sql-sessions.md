---
title: 'SQL sessions'
---

The implementation persists query metadata through OpenDAL and re-evaluates
results from one fixed Iceberg checkpoint. It does not require an RDB, queue,
or shared filesystem beyond the configured storage operator.

## Session metadata

`POST /spaces/{space_id}/sql-sessions` creates:

```text
spaces/{space_id}/sql_sessions/{session_id}/meta.json
```

The file contains:

```json
{
  "id": "session-uuid",
  "space_id": "space-main",
  "sql_id": "saved-or-generated-sql-id",
  "sql": "SELECT * FROM note ORDER BY _ugoite_updated_at, _ugoite_id",
  "parameters": {},
  "parameter_types": {},
  "authorized_principal_ids": ["principal-uuid"],
  "authorization_policy_hash": "sha256:…",
  "status": "ready",
  "created_at": "2026-03-11T10:10:00Z",
  "expires_at": "2026-03-11T10:20:00Z",
  "error": null,
  "checkpoint": {
    "format_version": 1,
    "space_id": "space-main",
    "catalog_generation": 42,
    "catalog_head_checksum": "sha256:…",
    "publication_location": "catalog/publications/42.json",
    "publication_checksum": "sha256:…",
    "form_registry_generation": 7,
    "tables": [],
    "coordinate_checksum": "sha256:…"
  },
  "pagination": {
    "strategy": "offset",
    "total_order": "ORDER BY ending with _ugoite_id",
    "default_limit": 50,
    "max_limit": 1000,
    "max_offset": 999
  },
  "limits": {"max_rows": 1000, "max_memory_bytes": 67108864, "timeout_ms": 30000, "max_concurrency": 1},
  "count": {"mode": "on_demand", "cached_at": null, "value": null}
}
```

Rows are not stored in the session directory. Status reads load the metadata
file; count and paged-row requests re-run the SQL against the same checkpoint
and the caller's current readable scope. The principal set and the current
authorization policy hash must still match the session; a changed policy is
rejected and requires a new session. There is no stream endpoint.

The initial page surface accepts only one Form relation and an explicit total
order ending in `_ugoite_id`. The API rejects joins, aggregates, `DISTINCT`,
subqueries, and page ranges beyond the retained 1,000-row session window.
Counts and pages remain bounded by the same DataFusion memory, timeout, and
single-query concurrency limits.

Sessions expire after ten minutes by default. Accessing an expired session marks it expired and returns an expiration error. A periodic cleanup worker is not shipped, so operators should not assume automatic background deletion.
