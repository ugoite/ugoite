# Directory Structure

## Machine-Readable Layout

`directory-layout.yaml` is the machine-readable source of truth for the paths in
this document. Tests read that YAML file and compare it against the filesystem
layout that `ugoite-core` actually creates.

## Space Layout

### Bootstrap Scaffold

Immediately after `create_space`, the runtime creates this scaffold:

```
spaces/
  {space_id}/                         # Each space is self-contained
    meta.json                         # Space metadata
    settings.json                     # Editor preferences and defaults
    forms/                            # Iceberg-managed root for Form tables
    assets/                           # Binary files (created lazily on upload)
    materialized_views/               # Materialized view root; individual views are lazy
    sql_sessions/                     # SQL query sessions (metadata only)
```

### Lazy Additions

The runtime adds these paths only when the corresponding feature is used:

| Trigger | Paths |
|---------|-------|
| Response signing | `spaces/{space_id}/hmac.json` |
| SQL session creation | `spaces/{space_id}/materialized_views/{sql_id}/meta.json`, `spaces/{space_id}/sql_sessions/{session_id}/meta.json` |
| Asset upload | `spaces/{space_id}/assets/*` |

## Space Level

### `meta.json`

`meta.json` is created during `create_space` and carries both the stable space
metadata and the per-space integrity key used for entry/revision signing.

```json
{
  "id": "space-main",
  "name": "space-main",
  "created_at": 1762000000.123,
  "storage": {
    "type": "local",
    "root": "/var/lib/ugoite"
  },
  "hmac_key_id": "key-6fe43d8d7b8842d0bca5d98976715f1a",
  "hmac_key": "base64-encoded-secret",
  "last_rotation": "2026-03-11T10:00:00Z"
}
```

### `settings.json`

```json
{
  "default_form": "Entry",
  "editor_theme": "dark",
  "sync_interval_seconds": 60
}
```

### `hmac.json`

`hmac.json` is created lazily the first time response signing is used. It is
separate from the integrity key fields stored in `meta.json`.

```json
{
  "hmac_key_id": "key-2026-03-11-response",
  "hmac_key": "base64-encoded-secret",
  "last_rotation": "2026-03-11T10:05:00Z"
}
```

## Form Tables (Iceberg)

### `forms/`

All form storage is managed by Apache Iceberg using the official Rust crate with
OpenDAL-backed IO. The filesystem layout **beneath this directory** is owned by
Iceberg and is intentionally not specified here. Each Form is represented as an
Iceberg namespace named by the Form name. Each Form namespace contains its own
`entries` and `revisions` tables (there is no shared cross-Form table).

**Template convention (global):**
```
# {form_name}

## {column_1}
## {column_2}
...
```
The template is fixed across the service; Form-specific templates are not stored
outside Iceberg.

**Required tables (logical names):**
- `entries` (current entry rows)
- `revisions` (revision history rows)

**Required operations:**
- Append new entry rows and update existing entry rows via Iceberg writes.
- Append revision rows for every save.
- Support snapshot/time-travel reads for conflict resolution and history.
- Allow compaction/maintenance via Iceberg without breaking logical access.

### `entries` table (logical schema)

One row per entry. Columns include standard metadata plus **only** the fields
defined by the Iceberg schema for that Form. The Form identity is implied by
the table name.

Example logical schema:

```text
entry_id: string
title: string
tags: list<string>
links: list<struct<id: string, target: string, kind: string>>
created_at: timestamp
updated_at: timestamp
fields: struct<...>
```

### `revisions` table (logical schema)

One row per revision. Stores historical snapshots of Form-defined fields so full
Markdown can be reconstructed deterministically.

Example logical schema:

```text
revision_id: string
entry_id: string
parent_revision_id: string
timestamp: timestamp
author: string
fields: struct<...>
markdown_checksum: string
```

## Portability

Each space directory is fully portable:
- Copy to another location to backup
- Move to different storage backend
- Share with other Ugoite instances

Materialized indexes (search, embeddings) are derived from Iceberg tables and can be regenerated.

## SQL Materialized Views (Metadata)

### `materialized_views/`

Saved SQL entries create a corresponding materialized view **metadata** record
under `materialized_views/`. The metadata is created on SQL creation, refreshed
on SQL update, and deleted on SQL deletion. The on-disk layout for future
Iceberg-managed views is intentionally not specified.

Example `materialized_views/{sql_id}/meta.json`:

```json
{
  "sql_id": "2e8de8f8-bb3c-48c8-9b59-5328de0b9cdd",
  "created_at": "2026-03-11T10:10:00Z",
  "updated_at": "2026-03-11T10:10:00Z",
  "snapshot_id": 123456789,
  "sql": "SELECT * FROM Entry.entries"
}
```

## SQL Sessions (Metadata Only)

SQL sessions persist **only metadata** to allow re-running queries against the
current entries tables. Result rows are never stored under `sql_sessions/`.

Example `sql_sessions/{session_id}/meta.json`:

```json
{
  "id": "efab8d7f-99c4-4bb8-b4a1-09c8f18df7b4",
  "space_id": "space-main",
  "sql_id": "2e8de8f8-bb3c-48c8-9b59-5328de0b9cdd",
  "sql": "SELECT * FROM Entry.entries",
  "status": "ready",
  "created_at": "2026-03-11T10:10:00Z",
  "expires_at": "2026-03-11T10:20:00Z",
  "error": null,
  "view": {
    "sql_id": "2e8de8f8-bb3c-48c8-9b59-5328de0b9cdd",
    "snapshot_id": 123456789,
    "snapshot_at": "2026-03-11T10:10:00Z",
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
