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

### `run_script`

The core power tool - executes JavaScript in a secure Wasm sandbox.

**Arguments**:
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `code` | string | Yes | JavaScript code to execute |
| `workspace_id` | string | Yes | Target workspace |

**Environment**:
- Runtime: WebAssembly (wasmtime) + QuickJS
- Security: Strictly sandboxed, no network/filesystem access
- Resource limits: Fuel (CPU cycles) and memory limits

**Host Interface**:

The global `host` object provides API access:

```javascript
// Call REST API from sandbox
host.call(method, path, body)

// Examples:
const notes = host.call("GET", `/workspaces/${workspace_id}/notes`);
const note = host.call("GET", `/workspaces/${workspace_id}/notes/${noteId}`);
const created = host.call("POST", `/workspaces/${workspace_id}/notes`, {
  markdown: "# New Note\n\nContent here"
});
```

**Example Usage**:

```javascript
// Find all pending tasks and create a summary
const tasks = host.call("POST", `/workspaces/${workspace_id}/query`, {
  filter: { class: "Task", "properties.Status": "pending" }
});

let report = "# Pending Tasks\n\n";
for (const task of tasks) {
  const due = task.properties.Due || "No due date";
  report += `- [ ] ${task.title} (Due: ${due})\n`;
}

// Return value is sent back to the AI
report;
```

**Error Handling**:

| Error | Description |
|-------|-------------|
| `FuelExhausted` | Script exceeded CPU cycle limit |
| `MemoryExceeded` | Script exceeded memory limit |
| `ExecutionError` | JavaScript runtime error |
| `HostCallError` | API call failed |

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

### Sandbox Isolation

1. **No Network**: Scripts cannot make external HTTP requests
2. **No Filesystem**: Scripts cannot read/write files directly
3. **API Only**: All data access goes through `host.call()` which uses the REST API
4. **Fuel Limits**: CPU cycles are metered to prevent infinite loops
5. **Memory Limits**: Wasm instances have bounded memory (128MB default)

### Authentication

MCP requests inherit the authentication of the HTTP connection:
- Localhost: No auth required
- Remote: Bearer token or API key required

### Audit Trail

All `run_script` executions are logged with:
- Timestamp
- Workspace ID
- Code hash (not full code for privacy)
- Result status
- Resource usage
