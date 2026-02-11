# Model Context Protocol (MCP) Specification

## Overview

Ugoite implements the Model Context Protocol (MCP) to enable AI agents to interact with the knowledge base. MCP requests are sent as HTTP POST to `/mcp`.

## Resources

Resources provide read-only access to data:

### `ugoite://{space_id}/entries/list`

Returns JSON list of entries with metadata.

```json
[
  {
    "id": "entry-uuid",
    "title": "Weekly Sync",
    "form": "Meeting",
    "properties": { "Date": "2025-11-29" },
    "updated_at": "2025-11-29T10:00:00Z"
  }
]
```

### `ugoite://{space_id}/entries/{entry_id}`

Returns Markdown content of a specific entry.

```markdown
# Weekly Sync

## Date
2025-11-29

## Attendees
- Alice
- Bob
```

### `ugoite://{space_id}/entries/{entry_id}/history`

Returns revision history summaries.

### `ugoite://{space_id}/forms`

Returns available form definitions and their fields.

### `ugoite://{space_id}/links`

Returns all entry-to-entry relationships.

---

## Tools

No MCP tools are currently exposed. The deprecated `run_script` tool has been removed.

---

## Prompts

Pre-defined prompts help AI understand the context:

### `summarize_space`

> "Read the index of the space and provide a high-level summary of the topics covered."

### `analyze_meetings`

> "Find all entries with form='Meeting' and summarize the key decisions."

### `find_related`

> "Given an entry ID, find all related entries via links and shared tags."

---

## Security Model

### Authentication

MCP requests inherit the authentication of the HTTP connection:
- Localhost: No auth required
- Remote: Bearer token or API key required

### Audit Trail

MCP requests are logged with:
- Timestamp
- Space ID
- Resource identifier
