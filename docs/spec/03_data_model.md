# 03. Data Model & Storage

This document expands on the storage promises introduced in `01_architecture.md` and the functional requirements in `02_features_and_stories.md`. It explains how a Local-First, AI-programmable workspace is represented on disk, how Markdown becomes structured data, and how APIs/MCP consume the resulting materialized views.

## 1. Storage Principles
*   **Filesystem = Database**: Every workspace is just a directory tree reachable through `fsspec` (local disk, S3, NAS). No hidden RDB or proprietary format.
*   **Class-on-Read, Class-on-Assist**: Markdown content stays flexible, but the Live Indexer (see `01_architecture.md`) projects it into typed objects whenever the frontend or an MCP agent needs structure.
*   **Hybrid Metadata Surface**:
    *   YAML Frontmatter captures high-level properties (class, status) needed before parsing body content.
    *   `## Section` blocks define strongly-typed fields (the "Structured Freedom" story from `02_features_and_stories.md`).
*   **Append-Only Integrity**: Writes never mutate history—new revisions are appended, signed, and indexed. Time-travel (Story 5) is therefore guaranteed.

## 2. Directory & File Inventory

The layout below is optimized for partial reads, cloud sync, and background indexing. Paths are relative to the configured storage root.

```
global.json
workspaces/
  {workspace_id}/
    meta.json
    settings.json              # Editor + workspace-level preferences
    schemas/
      {class}.json             # Class definitions (Meeting, Task, etc.)
    index/
      index.json               # Structured cache (materialized view)
      inverted_index.json      # Keyword posting lists
      faiss.index              # Vector store for semantic search
      stats.json               # Aggregates (counts, schema usage)
    attachments/
      ...                      # Binary large objects referenced from notes
    notes/
      {note_id}/
        meta.json
        content.json
        history/
          index.json           # Chronological list of revisions
          {revision_id}.json
```

### 2.1 Root-Level Artifacts
| File | Purpose |
|------|---------|
| `global.json` | Holds workspace registry, cryptographic seeds (HMAC), and default storage connectors. |
| `workspaces/` | Each subdirectory is fully portable; copying it elsewhere preserves the workspace. |

### 2.2 Workspace Artifacts
| File | Purpose |
|------|---------|
| `meta.json` | Canonical metadata (id, name, created_at, storage config, merge rules). |
| `settings.json` | Feature flags such as local-first sync intervals or default Class. |
| `schemas/*.json` | User-defined Classes that power template creation and validation. |
| `index/*` | Materialized views consumed by REST `/query`, `/search`, and MCP resources. |
| `notes/*` | Source of truth for all note content and history. |
| `attachments/*` | Binary payloads (audio, images) referenced by `content.json`. Stored outside note folders to simplify dedupe and large-file lifecycle. |

## 3. Metadata Layers & Extraction Rules

| Layer | Where it lives | Example | Notes |
|-------|----------------|---------|-------|
| Frontmatter | YAML block at top of Markdown | `class: meeting` | Parsed before Markdown; overrides default Class defaults. |
| Section Properties | H2 headers (`## Due Date`) | `## Agenda` + body text | Treated as top-level fields keyed by header text. Order is preserved for deterministic diffs. |
| Auto Properties | Computed | `word_count`, `embedding_id` | Populated by the indexer to support sorting and search. |

Conflicts between layers resolve with the following precedence: Section > Frontmatter > Auto default. The Live Indexer produces a single `properties` dict per note reflecting the merged view and stores it inside `index/index.json`.

### 3.1 Parsing Lifecycle
1. **Detect changes** via API writes (or internal filesystem watcher on `content.json`).
2. **Load Markdown** (from `content.json`) and extract frontmatter + body.
3. **Apply Class definition** (if note has `class`):
    * Validate required headers exist.
    * Cast value types (`date`, `number`, `list`).
    * Generate warnings surfaced in the frontend.
4. **Emit structured record** into `index/index.json` and refresh secondary indices (`faiss.index`, `inverted_index.json`).

## 4. Class Definitions & Templates

Classes formalize recurring note shapes (Meetings, Tasks, Research). They are plain JSON stored at `workspaces/{id}/schemas/{class}.json` and interpreted both by the editor (to render templates) and the MCP layer (to expose typed querying).

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

* **Validation Surface**: The frontend enforces Class definitions optimistically; the backend re-validates on write and exposes violations via `PUT /notes/{id}` responses (see `04_api_and_mcp.md`).
* **Template Insertion**: Creating a note with `class=Meeting` injects the template into the editor, satisfying Story 2.
* **Class Stats**: The indexer aggregates `class_stats` (counts per class, field cardinalities) into `index/stats.json` for fast filter UIs.

## 5. Structured Cache & Search Indices

The cache is a materialized view updated every time a `content.json` or `meta.json` changes. It feeds UI filters, REST `/query`, MCP `search_notes`, and AI code execution. The cache is split to keep hot paths light:

| File | Shape | Used by |
|------|-------|---------|
| `index/index.json` | `{ "notes": {id: NoteRecord}, "class_stats": {...} }` | REST `/workspaces/{id}/notes`, MCP resources, `ieapp.query()` |
| `index/inverted_index.json` | `{ term: [note_id, ...] }` | Keyword search fallback, offline search |
| `index/faiss.index` | Binary FAISS index referencing embeddings stored inside `NoteRecord.embedding_id` | Semantic search + MCP `search_notes` |
| `index/stats.json` | Aggregates (per-tag counts, last indexed timestamp) | Health checks, UI badges |

**NoteRecord shape**
```json
{
  "id": "note-uuid",
  "title": "Weekly Sync",
  "class": "meeting",
  "updated_at": "2025-11-29T10:00:00Z",
  "properties": {
    "Date": "2025-11-29",
    "Attendees": ["Alice", "Bob"],
    "Action Items": ["Alice to update the slide deck"]
  },
  "tags": ["project"],
  "links": [
    { "id": "link-123", "target": "note-uuid-2", "kind": "related" }
  ],
  "embedding_id": "emb-123",
  "checksum": "sha256-..."
}
```

## 6. File Schemas

### 6.1 `global.json`
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

### 6.2 Workspace Metadata `workspaces/{id}/meta.json`
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
  "default_class": "meeting",
  "encryption": { "mode": "none" }
}
```

### 6.3 Note Metadata `notes/{id}/meta.json`
```json
{
  "id": "note-uuid",
  "workspace_id": "ws-main",
  "title": "Weekly Sync",
  "class": "meeting",
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

### 6.4 Note Content `notes/{id}/content.json`
```json
{
  "revision_id": "rev-0042",
  "author": "frontend",
  "markdown": "# Weekly Sync\n\n## Date\n2025-11-29\n\n```python {id=block-xyz}\nprint('Hello')\n```",
  "frontmatter": {
    "class": "meeting",
    "status": "open"
  },
  "attachments": [
    { "name": "audio.m4a", "path": "attachments/a1b2c3d4e5f6..." }
  ],
  "computed": {
    "word_count": 523
  }
}
```
```

### 6.5 Revision Entry `notes/{id}/history/{revision_id}.json`
```json
{
  "revision_id": "rev-0042",
  "parent_revision_id": "rev-0041",
  "timestamp": "2025-11-29T10:00:00Z",
  "author": "frontend" ,
  "diff": "...optional patch...",
  "content_snapshot_path": "../content.json",
  "integrity": {
    "checksum": "sha256-...",
    "signature": "hmac-..."
  }
}
```

### 6.6 Revision Index `notes/{id}/history/index.json`
```json
{
  "note_id": "note-uuid",
  "revisions": [
    { "revision_id": "rev-0001", "timestamp": "2025-10-01T12:00:00Z" },
    { "revision_id": "rev-0042", "timestamp": "2025-11-29T10:00:00Z" }
  ]
}
```

## 7. Versioning & Conflict Resolution

The append-only strategy described in Story 5 and reiterated here underpins REST `PUT /notes/{id}` and MCP writes.

1. **Client reads** `content.json` obtaining `revision_id` (as the current head).
2. **Client writes** new content, providing `parent_revision_id` (which matches the read `revision_id`).
3. **Backend verifies** `parent_revision_id == content.revision_id`.
4. **On success**:
    * Generate new `revision_id` and persist `history/{revision_id}.json`.
    * Update `content.json` and `meta.json` atomically (same filesystem transaction when supported by backend FS; otherwise best-effort with retry and checksum validation).
    * Emit change event to indexer so caches refresh.
5. **On mismatch**: Return HTTP 409 with the server’s latest revision payload so the client (or MCP agent) can perform a 3-way merge triggered by the user.

## 8. Integrity, Security & Auditing

Security expectations from `05_security_and_quality.md` surface directly in the data model:

* **HMAC Signatures**: Every `meta.json`, `content.json`, and history file carries `integrity.signature`. The secret key lives in `global.json` (`hmac_key`) and can rotate (previous keys stored for validation during rotation).
* **Checksums**: SHA-256 of the canonical JSON string ensures bit-rot detection even when signatures are skipped (read-only contexts).
* **Audit Trail**: Optional `history/{revision_id}.json` can store `author` metadata stating whether the change came from frontend, API, or `run_python_script`. This supports the "AI wrote this" transparency requirement.
* **Isolation**: Attachments reference hashed filenames to prevent path traversal when notes are moved between workspaces.

## 9. Consumption Patterns (API & MCP)

* **REST** (`04_api_and_mcp.md`): `/workspaces/{ws}/notes` reads exclusively from `index/index.json` for sub-500 ms workspace load times. `/workspaces/{ws}/query` translates filters into index scans (structured) plus inverted-index lookups (keywords).
* **MCP Resources**: `ieapp://{ws}/notes/list` streams the same NoteRecord objects. Agents rely on these lightweight summaries before deciding to fetch `content.json` or run custom Python.
* **`run_python_script` Tool**: The `ieapp` library loads `index/index.json` lazily and exposes helper methods (e.g., `ieapp.query()`) so AI agents rarely need to traverse the filesystem manually—aligning with the "Code Execution" paradigm.

These consumers, combined with the storage rules above, ensure that data written anywhere (UI, CLI, MCP) is immediately queryable, conflict-safe, and portable.
