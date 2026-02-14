# Directory Structure

## Space Layout

```
hmac.json                             # Root: response-signing key material
spaces/
  {space_id}/                         # Each space is self-contained
    meta.json                         # Space metadata
    settings.json                     # Editor preferences, defaults
    forms/                            # Iceberg-managed root for Form tables
    assets/                           # Binary files (images, audio, etc.)
      {hash}.{ext}                    # Content-addressed storage
    materialized_views/               # SQL materialized view metadata (no rows)
    sql_sessions/                     # SQL query sessions (metadata only)
      {session_id}/                   # Session directory
        meta.json                     # Session metadata (status, snapshots)
```

## Root Level

### `hmac.json`

Root response-signing key material:

```json
{
  "hmac_key_id": "key-2025-11-01",
  "hmac_key": "base64-encoded-secret",
  "last_rotation": "2025-11-15T00:00:00Z"
}
```

## Space Level

### `meta.json`

```json
{
  "id": "space-main",
  "name": "Personal Knowledge",
  "created_at": "2025-08-12T12:00:00Z",
  "storage_config": {
    "uri": "s3://my-bucket/ugoite/space-main",
    "credentials_profile": "default"
  },
  "merge_strategy": "manual",
  "default_form": "Entry",
  "encryption": { "mode": "none" }
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

## SQL Sessions (Metadata Only)

SQL sessions persist **only metadata** to allow re-running queries against the
current entries tables. Result rows are never stored under `sql_sessions/`.
