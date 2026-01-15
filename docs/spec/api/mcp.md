# Model Context Protocol (MCP) Specification

## Overview

IEapp implements the Model Context Protocol (MCP) to enable AI agents to interact with the knowledge base. MCP requests are sent as HTTP POST to `/mcp`.

## Resources

Resources provide read-only access to data:

### `ieapp://{workspace_id}/notes/list`

Returns JSON list of notes with metadata.

```json
[
  {
    "id": "note-uuid",
    "title": "Weekly Sync",
    "class": "Meeting",
    "properties": { "Date": "2025-11-29" },
    "updated_at": "2025-11-29T10:00:00Z"
  }
]
```

### `ieapp://{workspace_id}/notes/{note_id}`

Returns Markdown content of a specific note.

```markdown
# Weekly Sync

## Date
2025-11-29

## Attendees
- Alice
- Bob
```

### `ieapp://{workspace_id}/notes/{note_id}/history`

Returns revision history summaries.

### `ieapp://{workspace_id}/classes`

Returns available class definitions and their fields.

### `ieapp://{workspace_id}/links`

Returns all note-to-note relationships.

---

## Tools

No MCP tools are currently exposed. The deprecated `run_script` tool has been removed.

---

## Prompts

Pre-defined prompts help AI understand the context:

### `summarize_workspace`

> "Read the index of the workspace and provide a high-level summary of the topics covered."

### `analyze_meetings`

> "Find all notes with class='Meeting' and summarize the key decisions."

### `find_related`

> "Given a note ID, find all related notes via links and shared tags."

---

## Security Model

### Authentication

MCP requests inherit the authentication of the HTTP connection:
- Localhost: No auth required
- Remote: Bearer token or API key required

### Audit Trail

MCP requests are logged with:
- Timestamp
- Workspace ID
- Resource identifier
