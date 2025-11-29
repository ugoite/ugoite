# 04. API & MCP Specification

## 1. REST API (Frontend Interface)

The REST API is used by the SolidJS frontend for interactive UI operations.

### Workspaces
*   `GET /workspaces` - List workspaces.
*   `POST /workspaces` - Create new workspace.
*   `GET /workspaces/{id}` - Get metadata.
*   `PATCH /workspaces/{id}` - Update workspace metadata (name, default class, storage connector URI/credentials).
*   `POST /workspaces/{id}/test-connection` - Validate the configured `fsspec` connector before committing changes.

### Schemas
*   `GET /workspaces/{ws_id}/schemas` - List class definitions (used by Story 2 templates).
*   `GET /workspaces/{ws_id}/schemas/{class}` - Fetch a specific schema.
*   `PUT /workspaces/{ws_id}/schemas/{class}` - Create or update a schema definition.
*   `DELETE /workspaces/{ws_id}/schemas/{class}` - Remove an unused schema (fails if notes still reference it).

### Notes
*   `GET /workspaces/{ws_id}/notes` - List notes (uses `index.json`).
*   `POST /workspaces/{ws_id}/notes` - Create note.
*   `GET /workspaces/{ws_id}/notes/{note_id}` - Get note content.
*   `PUT /workspaces/{ws_id}/notes/{note_id}` - Update note (requires `parent_revision_id`; persists markdown, structured properties, and `canvas_position`).
*   `DELETE /workspaces/{ws_id}/notes/{note_id}` - Tombstone note.
*   `GET /workspaces/{ws_id}/notes/{note_id}/history` - List revisions for Time Travel UI.
*   `GET /workspaces/{ws_id}/notes/{note_id}/history/{revision_id}` - Fetch a specific revision payload.
*   `POST /workspaces/{ws_id}/notes/{note_id}/restore` - Restore a revision (creates a new head revision referencing the chosen ancestor).
*   `POST /workspaces/{ws_id}/notes/{note_id}/blocks/{block_id}/execute` - Execute an embedded notebook/code block and persist the output artifact.

### Canvas Links
*   `GET /workspaces/{ws_id}/links` - List graph edges (bi-directional note relationships for the Canvas view).
*   `POST /workspaces/{ws_id}/links` - Create a link between two notes (`source`, `target`, `kind`).
*   `DELETE /workspaces/{ws_id}/links/{link_id}` - Remove a link (UI automatically prunes both directions).

### Attachments
*   `POST /workspaces/{ws_id}/attachments` - Upload a binary blob (voice memo, image).
    *   **Returns**: `{ "id": "hash...", "path": "attachments/hash..." }`.
    *   **Note**: To link this attachment to a note, add the returned object to the `attachments` list in a subsequent `PUT /notes/{note_id}` call.
*   `DELETE /workspaces/{ws_id}/attachments/{attachment_id}` - Permanently delete an unattached blob (garbage collection safety check enforced).

### Query (Structured Data)
*   `POST /workspaces/{ws_id}/query` - Execute a structured query against the index.
    *   **Body**: `{ "filter": { "class": "meeting", "date": { "$gt": "2025-01-01" } } }`
    *   **Returns**: List of note objects with extracted properties.

### Search
*   `GET /workspaces/{ws_id}/search?q=query` - Hybrid search (Vector + Keyword).

## 2. Model Context Protocol (MCP)

The MCP interface is used by AI agents (Claude, Copilot, etc.). IEapp implements the official **Streamable HTTP** transport: every MCP request is an HTTP POST with JSON or optional SSE responses for long-running operations.

### Resources
Expose notes as readable resources for the AI.
*   `ieapp://{workspace_id}/notes/list` - JSON list of notes.
*   `ieapp://{workspace_id}/notes/{note_id}` - Markdown content of a note.
*   `ieapp://{workspace_id}/notes/{note_id}/history` - Revision summaries for Time Travel or restoration flows.
*   `ieapp://{workspace_id}/schema` - Available properties and values (e.g., all used tags, types).
*   `ieapp://{workspace_id}/links` - Canvas graph edges (source, target, metadata).

### Tools
The core power of IEapp is the **Code Execution Tool**.

#### `run_python_script`
*   **Description**: Executes a Python script in the context of the workspace. The script has access to the `ieapp` library to manipulate notes.
*   **Arguments**:
    *   `code` (string): The Python code to execute.
    *   `workspace_id` (string): The target workspace.
*   **Capabilities**:
    *   Read all notes: `ieapp.list_notes()`, `ieapp.get_note(id)`.
    *   **Query Properties**: `ieapp.query(class="meeting", status="open")`.
    *   Analyze data: Use `pandas`, `numpy` (if available) to process note content.
    *   Batch update: `ieapp.update_note(id, content)`.
    *   **Example Usage**:
        ```python
        # AI generates this code to find all notes tagged 'todo' and compile a report
        import ieapp
        
        # Query structured data extracted from Markdown
        # Note: Properties are extracted from H2 headers (e.g., ## Date)
        # Returns NoteRecord objects as defined in 03_data_model.md
        tasks = ieapp.query(class="task", status="pending")
        
        report = "# Pending Tasks Report\n"
        for task in tasks:
            # task is a NoteRecord with id, title, class, properties, etc.
            # Access extracted fields from the properties dict
            due_date = task.properties.get('Due', 'N/A')
            report += f"- [ ] {task.title} (Due: {due_date})\n"
            
        print(report)
        ```

#### `search_notes`
*   **Description**: Semantic search for notes.
*   **Arguments**: `query` (string).

#### `notes.list`
*   **Description**: Returns paginated note summaries (id, title, class, tags, canvas position) sourced from `index/index.json`.
*   **Arguments**:
    *   `workspace_id` (string): Target workspace.
    *   `filter` (object, optional): Same shape as REST `/query` filters for type/tag/date constraints.

#### `notes.read`
*   **Description**: Fetches a full note payload (frontmatter, markdown, attachments, latest revision id).
*   **Arguments**: `workspace_id`, `note_id`.

#### `notes.create`
*   **Description**: Creates a note from raw markdown or a schema template.
*   **Arguments**:
    *   `workspace_id`
    *   `title`
    *   `markdown`
    *   `class` (optional)
    *   `tags` (optional array)

#### `notes.update`
*   **Description**: Updates an existing note. Mirrors REST `PUT` semantics, requiring optimistic concurrency via `parent_revision_id`.
*   **Arguments**:
    *   `workspace_id`
    *   `note_id`
    *   `parent_revision_id`
    *   `markdown` / `frontmatter` / `properties` delta (at least one field)
    *   Optional `canvas_position`, `links`

#### `notes.delete`
*   **Description**: Tombstones a note (soft delete) so the UI can confirm before purging.
*   **Arguments**: `workspace_id`, `note_id`.

#### `notes.restore`
*   **Description**: Restores a past revision, creating a new head revision for auditing. Supports the Time Travel UI and agent-driven undo flows.
*   **Arguments**: `workspace_id`, `note_id`, `revision_id`.

### Prompts
Pre-defined prompts to help the AI understand the context.
*   `summarize_workspace`: "Read the index of the workspace and provide a high-level summary of the topics covered."
*   `analyze_meetings`: "Find all notes with class='meeting' and summarize the key decisions."
