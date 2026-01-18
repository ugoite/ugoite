# Directory Structure

## Workspace Layout

```
global.json                           # Root: workspace registry
workspaces/
  {workspace_id}/                     # Each workspace is self-contained
    meta.json                         # Workspace metadata
    settings.json                     # Editor preferences, defaults
    classes/                          # Class-first storage
      {class_id}/                     # One directory per Class
        class.json                    # Class definition (schema + template)
        tables/                       # Parquet-backed storage
          notes.parquet               # Current notes (one row per note)
          revisions.parquet           # Revision history (one row per revision)
    index/                            # Materialized views (regeneratable)
      index.json                      # Structured note records
      inverted_index.json             # Keyword search index
      stats.json                      # Aggregate statistics
      faiss.index                     # Vector embeddings (optional)
    attachments/                      # Binary files (images, audio, etc.)
      {hash}.{ext}                    # Content-addressed storage
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

## Class Level

### `classes/{class_id}/class.json`

```json
{
  "id": "class-uuid",
  "name": "Meeting",
  "version": 1,
  "template": "# Meeting\n\n## Date\n## Attendees\n## Decisions\n",
  "fields": {
    "Date": { "type": "date", "required": true },
    "Attendees": { "type": "list", "required": false },
    "Decisions": { "type": "markdown", "required": false }
  },
  "defaults": {
    "timezone": "UTC"
  }
}
```

## Class Tables (Parquet)

### `classes/{class_id}/tables/notes.parquet`

One row per note. Columns include standard metadata plus **only** the Class-defined fields.

Example logical schema:

```text
note_id: string
title: string
class_id: string
tags: list<string>
links: list<struct<id: string, target: string, kind: string>>
canvas_position: struct<x: double, y: double>
created_at: timestamp
updated_at: timestamp
fields: struct<
  Date: date,
  Attendees: list<string>,
  Decisions: string
>
```

### `classes/{class_id}/tables/revisions.parquet`

One row per revision. Stores historical snapshots of Class-defined fields so full
Markdown can be reconstructed deterministically.

Example logical schema:

```text
revision_id: string
note_id: string
parent_revision_id: string
timestamp: timestamp
author: string
fields: struct<
  Date: date,
  Attendees: list<string>,
  Decisions: string
>
markdown_checksum: string
```

## Index Level

### `index/index.json`

```json
{
  "notes": {
    "note-uuid": {
      "id": "note-uuid",
      "title": "Weekly Sync",
      "class": "Meeting",
      "updated_at": "2025-11-29T10:00:00Z",
      "properties": {
        "Date": "2025-11-29",
        "Attendees": ["Alice", "Bob"]
      },
      "tags": ["project-alpha"],
      "links": [{ "id": "link-456", "target": "note-uuid-2", "kind": "related" }],
      "checksum": "sha256-..."
    }
  },
  "class_stats": {
    "Meeting": { "count": 15, "last_updated": "2025-11-29T10:00:00Z" }
  }
}
```

### `index/stats.json`

```json
{
  "last_indexed": 1732878000.0,
  "note_count": 150,
  "tag_counts": {
    "project-alpha": 25,
    "important": 10
  }
}
```

## Portability

Each workspace directory is fully portable:
- Copy to another location to backup
- Move to different storage backend
- Share with other IEapp instances

The `index/` directory can be regenerated from Parquet tables, so it's safe to exclude from backups if needed.
