# Directory Structure

## Workspace Layout

```
global.json                           # Root: workspace registry
workspaces/
  {workspace_id}/                     # Each workspace is self-contained
    meta.json                         # Workspace metadata
    settings.json                     # Editor preferences, defaults
    classes/                          # Iceberg-managed root for Class tables
    attachments/                      # Binary files (images, audio, etc.)
      {hash}.{ext}                    # Content-addressed storage
    sql_sessions/                     # SQL query sessions
      {session_id}/                   # Session directory
        meta.json                     # Session metadata (status, progress)
        rows.json                     # Stored result rows
```

## Root Level

### `global.json`

Workspace registry and system configuration:

```json
{
  "version": 1,
  "default_storage": "fs:///Users/alex/ieapp",
  "workspaces": ["ws-main", "ws-research"],
  "hmac_key_id": "key-2025-11-01",
  "hmac_key": "base64-encoded-secret",
  "last_rotation": "2025-11-15T00:00:00Z"
}
```

## Workspace Level

### `meta.json`

```json
{
  "id": "ws-main",
  "name": "Personal Knowledge",
  "created_at": "2025-08-12T12:00:00Z",
  "storage_config": {
    "uri": "s3://my-bucket/ieapp/ws-main",
    "credentials_profile": "default"
  },
  "merge_strategy": "manual",
  "default_class": "Note",
  "encryption": { "mode": "none" }
}
```

### `settings.json`

```json
{
  "default_class": "Note",
  "editor_theme": "dark",
  "sync_interval_seconds": 60
}
```

## Class Tables (Iceberg)

### `classes/`

All class storage is managed by Apache Iceberg using the official Rust crate with
OpenDAL-backed IO. The filesystem layout **beneath this directory** is owned by
Iceberg and is intentionally not specified here. Each Class is represented as an
Iceberg namespace named by the Class name. Each Class namespace contains its own
`notes` and `revisions` tables (there is no shared cross-Class table).

**Template convention (global):**
```
# {class_name}

## {column_1}
## {column_2}
...
```
The template is fixed across the service; Class-specific templates are not stored
outside Iceberg.

**Required tables (logical names):**
- `notes` (current note rows)
- `revisions` (revision history rows)

**Required operations:**
- Append new note rows and update existing note rows via Iceberg writes.
- Append revision rows for every save.
- Support snapshot/time-travel reads for conflict resolution and history.
- Allow compaction/maintenance via Iceberg without breaking logical access.

### `notes` table (logical schema)

One row per note. Columns include standard metadata plus **only** the fields
defined by the Iceberg schema for that Class. The Class identity is implied by
the table name.

Example logical schema:

```text
note_id: string
title: string
tags: list<string>
links: list<struct<id: string, target: string, kind: string>>
canvas_position: struct<x: double, y: double>
created_at: timestamp
updated_at: timestamp
fields: struct<...>
```

### `revisions` table (logical schema)

One row per revision. Stores historical snapshots of Class-defined fields so full
Markdown can be reconstructed deterministically.

Example logical schema:

```text
revision_id: string
note_id: string
parent_revision_id: string
timestamp: timestamp
author: string
fields: struct<...>
markdown_checksum: string
```

## Portability

Each workspace directory is fully portable:
- Copy to another location to backup
- Move to different storage backend
- Share with other IEapp instances

Materialized indexes (search, embeddings) are derived from Iceberg tables and can be regenerated.
