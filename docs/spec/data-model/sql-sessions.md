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
  "sql": "SELECT * FROM note ORDER BY updated_at, id",
  "status": "ready",
  "created_at": "2026-03-11T10:10:00Z",
  "expires_at": "2026-03-11T10:20:00Z",
  "error": null,
  "checkpoint": {"catalog_generation": 42, "tables": []},
  "pagination": {
    "strategy": "offset",
    "order_by": ["updated_at", "id"],
    "default_limit": 50,
    "max_limit": 1000
  },
  "count": {"mode": "on_demand", "cached_at": null, "value": null}
}
```

Rows are not stored in the session directory. Status reads load the metadata
file; count and paged-row requests re-run the SQL against the checkpoint and
the caller's current readable scope. There is no stream endpoint.

Sessions expire after ten minutes by default. Accessing an expired session marks it expired and returns an expiration error. A periodic cleanup worker is not shipped, so operators should not assume automatic background deletion.
