# 03. Data Model & Storage

## 1. Storage Philosophy
*   **No Database**: The file system is the database.
*   **Schema-on-Read**: Structure is defined by the content, not a rigid schema.
*   **Hybrid Data**: 
    *   **Explicit Metadata**: YAML Frontmatter (Page-level properties).
    *   **Inline Metadata**: `Key:: Value` syntax within the text (Block-level properties).
*   **Immutable History**: Files are never overwritten; new revisions are appended.

## 2. Directory Structure
The storage layout is designed for `fsspec` compatibility and efficient partial reads.

```
{root}/
├── global.json                     # Global config, HMAC keys (protected)
├── workspaces/
│   ├── {workspace_id}/
│   │   ├── meta.json               # Workspace metadata (name, created_at)
│   │   ├── index.json              # Structured Cache (All properties extracted)
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

## 3. Structured Markdown Syntax

IEapp parses Markdown headers to extract structured data.

### 3.1. Section-Based Properties
Instead of proprietary inline syntax, IEapp treats **H2 Headers** as property keys and their content as values. This is standard Markdown, readable by any tool.

**Example Note:**
```markdown
# Weekly Sync

## Date
2025-11-29

## Attendees
- Alice
- Bob

## Agenda
1. Review Q3 goals
2. Plan Q4 roadmap

## Action Items
- [ ] Alice to update the slide deck
```

**Extracted Data:**
```json
{
  "Date": "2025-11-29",
  "Attendees": ["Alice", "Bob"],
  "Agenda": "1. Review Q3 goals\n2. Plan Q4 roadmap",
  "Action Items": "- [ ] Alice to update the slide deck"
}
```

### 3.2. Class & Schema Definition
Users can define "Classes" (e.g., Meeting, Report) to enforce structure.

**Schema Definition (`workspaces/{id}/schemas/meeting.json`):**
```json
{
  "name": "Meeting",
  "template": "# New Meeting\n\n## Date\n\n## Attendees\n",
  "fields": {
    "Date": { "type": "date", "required": true },
    "Attendees": { "type": "list", "required": false }
  }
}
```

*   **Validation**: The Frontend checks if the note content matches the schema (e.g., "Date" section exists and is a valid date).
*   **Templates**: Creating a new "Meeting" note pre-fills the editor with the H2 headers defined in the schema.

## 4. The Structured Cache (`index.json`)

The `index.json` is NOT just a list of files. It is a **Materialized View** of all structured data extracted from the notes. This allows O(1) querying without parsing files.

```json
{
  "notes": {
    "note-uuid-1": {
      "id": "note-uuid-1",
      "title": "Weekly Sync",
      "updated_at": "2025-11-29T10:00:00Z",
      "properties": {
        "Date": "2025-11-29",
        "Attendees": ["Alice", "Bob"],
        "Agenda": "1. Review Q3 goals\n2. Plan Q4 roadmap",
        "Action Items": "- [ ] Alice to update the slide deck"
      }
    }
  },
  "schema_stats": {
    "type": ["meeting", "idea"],
    "status": ["open", "closed"]
  }
}
```

## 5. JSON Schemas (Storage)

### Workspace Metadata (`workspaces/{id}/meta.json`)
```json
{
  "id": "uuid-string",
        "attendees": ["Alice", "Bob"],
        "project": "Apollo",
        "due": "2025-12-15",
        "budget": "$5000"
      }
    }
  },
  "schema_stats": {
    "type": ["meeting", "idea"],
    "status": ["open", "closed"]
  }
}
```

## 5. JSON Schemas (Storage)

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
