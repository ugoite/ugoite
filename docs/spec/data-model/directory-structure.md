# Directory Structure

## Workspace Layout

```
global.json                           # Root: workspace registry
workspaces/
  {workspace_id}/                     # Each workspace is self-contained
    meta.json                         # Workspace metadata
    settings.json                     # Editor preferences, defaults
    classes/                          # Class definitions (note types)
      Meeting.json
      Task.json
      Note.json                       # Default class
    index/                            # Materialized views (regeneratable)
      index.json                      # Structured note records
      inverted_index.json             # Keyword search index
      stats.json                      # Aggregate statistics
      faiss.index                     # Vector embeddings (optional)
    attachments/                      # Binary files (images, audio, etc.)
      {hash}.{ext}                    # Content-addressed storage
    notes/
      {note_id}/                      # Each note has its own directory
        meta.json                     # Note metadata (title, tags, links)
        content.json                  # Markdown content + frontmatter
        history/                      # Revision history
          index.json                  # List of all revisions
          {revision_id}.json          # Individual revision (with diff)
```

## Root Level

### `global.json`

Workspace registry and system configuration:

```json
{
  "version": 1,
  "default_storage": "file:///Users/alex/ieapp",
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

### `classes/{name}.json`

```json
{
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

## Note Level

### `notes/{id}/meta.json`

```json
{
  "id": "note-uuid",
  "workspace_id": "ws-main",
  "title": "Weekly Sync",
  "class": "Meeting",
  "tags": ["project-alpha"],
  "links": [
    { "id": "link-456", "target": "note-uuid-2", "kind": "related" }
  ],
  "canvas_position": { "x": 120, "y": 480 },
  "created_at": "2025-11-20T09:00:00Z",
  "updated_at": "2025-11-29T10:00:00Z",
  "integrity": {
    "checksum": "sha256-...",
    "signature": "hmac-..."
  }
}
```

### `notes/{id}/content.json`

```json
{
  "revision_id": "rev-0042",
  "author": "frontend",
  "markdown": "# Weekly Sync\n\n## Date\n2025-11-29\n\n## Attendees\n- Alice\n- Bob",
  "frontmatter": {
    "class": "Meeting",
    "status": "open"
  },
  "attachments": [
    { "id": "a1b2c3d4", "name": "audio.m4a", "path": "attachments/a1b2c3d4.m4a" }
  ],
  "computed": {
    "word_count": 52
  }
}
```

### `notes/{id}/history/index.json`

```json
{
  "note_id": "note-uuid",
  "revisions": [
    { "revision_id": "rev-0001", "timestamp": "2025-10-01T12:00:00Z" },
    { "revision_id": "rev-0042", "timestamp": "2025-11-29T10:00:00Z" }
  ]
}
```

### `notes/{id}/history/{revision_id}.json`

```json
{
  "revision_id": "rev-0042",
  "parent_revision_id": "rev-0041",
  "timestamp": "2025-11-29T10:00:00Z",
  "author": "frontend",
  "diff": "--- a/content.md\n+++ b/content.md\n@@ -1,3 +1,4 @@...",
  "integrity": {
    "checksum": "sha256-...",
    "signature": "hmac-..."
  }
}
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

The `index/` directory can be regenerated from notes, so it's safe to exclude from backups if needed.
