# 04. API & MCP Specification

## 1. REST API (Frontend Interface)

The REST API is used by the SolidJS frontend for interactive UI operations.

### Workspaces
*   `GET /workspaces` - List workspaces.
*   `POST /workspaces` - Create new workspace.
*   `GET /workspaces/{id}` - Get metadata.
*   `PATCH /workspaces/{id}` - Update workspace metadata and settings.
    *   **Payload**: `{ "name": "...", "storage_config": {...}, "settings": {...} }`.
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
*   `PUT /workspaces/{ws_id}/notes/{note_id}` - Update note (requires `parent_revision_id`).
    *   **Payload**: `{ "markdown": "...", "frontmatter": {...}, "canvas_position": {...} }`.
    *   **Note**: Structured properties are NOT updated directly. To change a property (e.g., "Date"), the client must update the corresponding Markdown header (e.g., `## Date`) in the `markdown` field.
*   `DELETE /workspaces/{ws_id}/notes/{note_id}` - Tombstone note.
*   `GET /workspaces/{ws_id}/notes/{note_id}/history` - List revisions for Time Travel UI.
*   `GET /workspaces/{ws_id}/notes/{note_id}/history/{revision_id}` - Fetch a specific revision payload.
*   `POST /workspaces/{ws_id}/notes/{note_id}/restore` - Restore a revision (creates a new head revision referencing the chosen ancestor).
*   `POST /workspaces/{ws_id}/notes/{note_id}/blocks/{block_id}/execute` - Execute an embedded notebook/code block and persist the output artifact.

### Canvas Links
*   `GET /workspaces/{ws_id}/links` - List all graph edges across notes (aggregated from each note's `meta.json`).
*   `POST /workspaces/{ws_id}/links` - Create a link between two notes. Updates both notes' `meta.json` files.
    *   **Payload**: `{ "source": "note-id-1", "target": "note-id-2", "kind": "related" }`
*   `DELETE /workspaces/{ws_id}/links/{link_id}` - Remove a link. Updates both notes' `meta.json` files.

### Attachments
*   `POST /workspaces/{ws_id}/attachments` - Upload a binary blob (voice memo, image).
    *   **Returns**: `{ "id": "hash...", "name": "original-filename.m4a", "path": "attachments/hash..." }`.
    *   **Note**: To link this attachment to a note, add the returned object to the `attachments` array in a subsequent `PUT /notes/{note_id}` call.
*   `DELETE /workspaces/{ws_id}/attachments/{attachment_id}` - Permanently delete an unattached blob (garbage collection safety check enforced).

### Query (Structured Data)
*   `POST /workspaces/{ws_id}/query` - Execute a structured query against the index.
    *   **Body**: `{ "filter": { "class": "meeting", "date": { "$gt": "2025-01-01" } } }`
    *   **Returns**: List of note objects with extracted properties.

### Search
*   `GET /workspaces/{ws_id}/search?q=query` - Hybrid search (Vector + Keyword).

## 2. Model Context Protocol (MCP)

The MCP interface is used by AI agents (Claude, Copilot, etc.). IEapp implements MCP over HTTP: MCP requests are sent as HTTP POST to the backend's MCP endpoint (`/mcp`).

### Resources
Expose notes as readable resources for the AI.
*   `ieapp://{workspace_id}/notes/list` - JSON list of notes.
*   `ieapp://{workspace_id}/notes/{note_id}` - Markdown content of a note.
*   `ieapp://{workspace_id}/notes/{note_id}/history` - Revision summaries for Time Travel or restoration flows.
*   `ieapp://{workspace_id}/schema` - Available properties and values (e.g., all used tags, types).
*   `ieapp://{workspace_id}/links` - Canvas graph edges (source, target, metadata).

### Tools
The core power of IEapp is the **Universal Code Execution Tool**. To simplify the interface and maximize flexibility, IEapp exposes a **single** MCP tool that allows the AI to interact with the system by writing and executing scripts.

#### `run_script`
*   **Description**: Executes a JavaScript script in a secure WebAssembly (Wasm) sandbox. The script has access to the host application's REST API via a host function, allowing it to perform any action (CRUD notes, search, query, etc.) that the frontend can perform.
*   **Arguments**:
    *   `code` (string): The JavaScript code to execute.
    *   `workspace_id` (string): The target workspace.
*   **Environment**:
    *   **Runtime**: WebAssembly (wasmtime) running a JavaScript engine (e.g., QuickJS).
    *   **Security**: Strictly sandboxed. No network/filesystem access except via the provided host function.
*   **Host Interface**:
    *   The global object `host` is available.
    *   `host.call(method, path, body)`: Calls an internal API endpoint.
        *   `method`: HTTP verb ("GET", "POST", "PUT", "DELETE", "PATCH").
        *   `path`: The API path (e.g., `/workspaces/{id}/notes`).
        *   `body`: JSON object (optional).
    *   **Returns**: The JSON response from the API.
*   **Capabilities**:
    *   **Dynamic API Discovery**: The available operations are dynamically derived from the application's REST API definition (OpenAPI).
    *   **Complex Workflows**: The script can make multiple API calls, process data, and return a synthesized result.
*   **Example Usage**:
    ```javascript
    // AI generates this code to find all notes tagged 'todo' and compile a report
    
    // 1. Search/Query for notes
    // Corresponds to POST /workspaces/{ws_id}/query
    const tasks = host.call("POST", `/workspaces/${workspace_id}/query`, {
        filter: { class: "task", status: "pending" }
    });

    let report = "# Pending Tasks Report\n";
    
    for (const task of tasks) {
        // 2. Fetch full content if needed (or just use metadata)
        // const fullNote = host.call("GET", `/workspaces/${workspace_id}/notes/${task.id}`);
        
        const dueDate = task.properties.Due || 'N/A';
        report += `- [ ] ${task.title} (Due: ${dueDate})\n`;
    }
    
    // The return value of the script is returned to the AI
    report; 
    ```

### Prompts
Pre-defined prompts to help the AI understand the context.
*   `summarize_workspace`: "Read the index of the workspace and provide a high-level summary of the topics covered."
*   `analyze_meetings`: "Find all notes with class='meeting' and summarize the key decisions."
