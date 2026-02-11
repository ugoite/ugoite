````chatagent
# SQL Sessions & Materialized Views

**Updated**: 2026-02

This document defines how IEapp manages SQL execution without persisting
large result sets. The design is **stateless except for OpenDAL storage** and
avoids RDBs, external job queues, or NFS-based shared disks.

## Constraints

- **No RDB for session state** (no PostgreSQL/MySQL/etc.).
- **No external job queue** (Celery/Redis/RabbitMQ, etc.).
- **No NFS shared disk**. OpenDAL storage is the only shared persistence.
- **Short-lived sessions** (target: ~10 minutes).
- **Multiple API servers** may serve the same session concurrently.

## Materialized Views

When a saved SQL is created (`create_sql`), a corresponding **materialized view**
MUST be created under:

```
spaces/{space_id}/materialized_views/{sql_id}/
```

The materialized view lifecycle is **synchronized** with the SQL entry:

- **Create SQL** → create materialized view
- **Update SQL** → refresh/rebuild materialized view
- **Delete SQL** → delete materialized view

Materialized views are **Iceberg-managed** tables. The on-disk layout is owned
by Iceberg and intentionally not specified.

### Materialized View Metadata (Optional)

A lightweight metadata file may be stored at:

```
spaces/{space_id}/materialized_views/{sql_id}/meta.json
```

Recommended fields:

```json
{
  "sql_id": "sql-uuid",
  "created_at": "2026-02-10T12:00:00Z",
  "updated_at": "2026-02-10T12:05:00Z",
  "snapshot_id": 42,
  "schema_hash": "sha256:..."
}
```

## SQL Sessions (Metadata Only)

SQL sessions store **metadata only** at:

```
spaces/{space_id}/sql_sessions/{session_id}/meta.json
```

Sessions do **not** store result rows. Every query for rows/count re-runs the SQL
against the **materialized view snapshot** captured in the session metadata.

### Session Metadata Schema (Recommended)

```json
{
  "id": "session-uuid",
  "space_id": "space-uuid",
  "sql_id": "sql-uuid",
  "sql": "SELECT * FROM Meeting ORDER BY updated_at DESC LIMIT 50",
  "status": "ready",
  "created_at": "2026-02-10T12:00:00Z",
  "expires_at": "2026-02-10T12:10:00Z",
  "error": null,
  "view": {
    "sql_id": "sql-uuid",
    "snapshot_id": 42,
    "snapshot_at": "2026-02-10T12:00:00Z",
    "schema_version": 1
  },
  "pagination": {
    "strategy": "offset",
    "order_by": ["updated_at", "id"],
    "default_limit": 50,
    "max_limit": 1000
  },
  "count": {
    "mode": "on_demand",
    "cached_at": null,
    "value": null
  }
}
```

### Paging & Count

- **Rows**: `offset`/`limit` is applied to the materialized view using the
  snapshot pinned in `meta.json`.
- **Count**: computed on-demand with `SELECT COUNT(*)` against the same snapshot.
- **Fast paging**: `order_by` in metadata MUST include a deterministic tie-breaker
  (e.g., `id`) to avoid unstable pages.

### Expiration & Cleanup

Sessions are short-lived. Implementations SHOULD delete expired session metadata
on a periodic sweep or when accessed after `expires_at`.

## Multi-Server Behavior

Because the only persistence is OpenDAL storage, **any API server** can service
requests for the same session by reading `meta.json`, then querying the view
snapshot referenced in `view.snapshot_id`.

````
