# 03. Data Model & Storage

## 1. Storage Philosophy
*   **No Database**: The file system is the database.
*   **Human Readable**: JSON is preferred over binary formats for metadata and content.
*   **Immutable History**: Files are never overwritten; new revisions are appended.

## 2. Directory Structure
The storage layout is designed for `fsspec` compatibility and efficient partial reads.

```
{root}/
├── global.json                     # Global config, HMAC keys (protected)
├── workspaces/
│   ├── {workspace_id}/
│   │   ├── meta.json               # Workspace metadata (name, created_at)
│   │   ├── index.json              # Lightweight list of all notes (ID, Title, Latest Rev)
│   │   ├── faiss.index             # Vector index for semantic search
│   │   ├── inverted_index.json     # Keyword search index
│   │   ├── notes/
│   │   │   ├── {note_id}/
│   │   │   │   ├── meta.json       # Note metadata (tags, canvas_pos)
│   │   │   │   ├── content.json    # Current content snapshot
│   │   │   │   └── history/
│   │   │   │       ├── {rev_id}.json  # Full snapshot of past revision
│   │   │   │       └── index.json     # List of revisions
```

## 3. JSON Schemas

### Workspace Metadata (`workspaces/{id}/meta.json`)
```json
{
  "id": "uuid-string",
  "name": "My Knowledge Base",
  "created_at": "ISO-8601",
  "storage_config": { ... },
  "merge_strategy": "ours|theirs|manual"
}
```

### Note Content (`notes/{id}/content.json` & History)
```json
{
  "id": "uuid-string",
  "workspace_id": "uuid-string",
  "revision_id": "uuid-string",
  "parent_revision_id": "uuid-string",
  "title": "Project Alpha",
  "content": "# Markdown Content...",
  "tags": ["project", "urgent"],
  "links": ["other-note-id"],
  "canvas_position": { "x": 100, "y": 200 },
  "created_at": "ISO-8601",
  "author": "user-or-agent-id",
  "checksum": "sha256-hash"
}
```

### Note Index (`workspaces/{id}/index.json`)
*Optimized for listing without opening every note file.*
```json
[
  {
    "id": "uuid-string",
    "title": "Project Alpha",
    "updated_at": "ISO-8601",
    "latest_revision_id": "uuid-string",
    "tags": ["project"]
  },
  ...
]
```

## 4. Versioning & Conflict Resolution

### Append-Only Strategy
1.  **Read**: Client reads `content.json` (which is a copy of the latest revision).
2.  **Write**:
    *   Client sends new content + `parent_revision_id`.
    *   Backend checks if `parent_revision_id` matches current latest.
    *   **If Match**:
        *   Generate new `revision_id`.
        *   Write `history/{new_rev}.json`.
        *   Atomically update `content.json` and `meta.json`.
    *   **If Mismatch** (Conflict):
        *   Backend rejects write (409 Conflict).
        *   Client must fetch latest, merge (3-way), and retry.

### Integrity
*   **HMAC**: All revision files are signed with a key stored in `global.json`.
*   **Checksums**: `content` hash is stored in metadata to detect bit rot.
