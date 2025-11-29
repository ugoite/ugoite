# 04. API & MCP Specification

## 1. REST API (Frontend Interface)

The REST API is used by the SolidJS frontend for interactive UI operations.

### Workspaces
*   `GET /workspaces` - List workspaces.
*   `POST /workspaces` - Create new workspace.
*   `GET /workspaces/{id}` - Get metadata.

### Notes
*   `GET /workspaces/{ws_id}/notes` - List notes (uses `index.json`).
*   `POST /workspaces/{ws_id}/notes` - Create note.
*   `GET /workspaces/{ws_id}/notes/{note_id}` - Get note content.
*   `PUT /workspaces/{ws_id}/notes/{note_id}` - Update note (requires `parent_rev_id`).
*   `DELETE /workspaces/{ws_id}/notes/{note_id}` - Tombstone note.

### Query (Structured Data)
*   `POST /workspaces/{ws_id}/query` - Execute a structured query against the index.
    *   **Body**: `{ "filter": { "type": "meeting", "date": { "$gt": "2025-01-01" } } }`
    *   **Returns**: List of note objects with extracted properties.

### Search
*   `GET /workspaces/{ws_id}/search?q=query` - Hybrid search (Vector + Keyword).

## 2. Model Context Protocol (MCP)

The MCP interface is used by AI agents (Claude, Copilot, etc.).

### Resources
Expose notes as readable resources for the AI.
*   `ieapp://{workspace_id}/notes/list` - JSON list of notes.
*   `ieapp://{workspace_id}/notes/{note_id}` - Markdown content of a note.
*   `ieapp://{workspace_id}/schema` - Available properties and values (e.g., all used tags, types).

### Tools
The core power of IEapp v2 is the **Code Execution Tool**.

#### `run_python_script`
*   **Description**: Executes a Python script in the context of the workspace. The script has access to the `ieapp` library to manipulate notes.
*   **Arguments**:
    *   `code` (string): The Python code to execute.
    *   `workspace_id` (string): The target workspace.
*   **Capabilities**:
    *   Read all notes: `ieapp.list_notes()`, `ieapp.get_note(id)`.
    *   **Query Properties**: `ieapp.query(type="meeting", status="open")`.
    *   Analyze data: Use `pandas`, `numpy` (if available) to process note content.
    *   Batch update: `ieapp.update_note(id, content)`.
    *   **Example Usage**:
        ```python
        # AI generates this code to find all notes tagged 'todo' and compile a report
        import ieapp
        
        # Query structured data extracted from Markdown
        # Note: Properties are extracted from H2 headers (e.g., ## Date)
        tasks = ieapp.query(type="task", status="pending")
        
        report = "# Pending Tasks Report\n"
        for task in tasks:
            # Access extracted fields directly
            report += f"- [ ] {task.title} (Due: {task.properties.get('Due')})\n"
            
        print(report)
        ```

#### `search_notes`
*   **Description**: Semantic search for notes.
*   **Arguments**: `query` (string).

### Prompts
Pre-defined prompts to help the AI understand the context.
*   `summarize_workspace`: "Read the index of the workspace and provide a high-level summary of the topics covered."
*   `analyze_meetings`: "Find all notes with type='meeting' and summarize the key decisions."

## 3. Code Sandbox Security
Since `run_python_script` is powerful, it must be sandboxed.
*   **Network Access**: Blocked (except to `fsspec` storage if remote).
*   **Filesystem Access**: Restricted to `/tmp` and the `fsspec` mount.
*   **Timeouts**: Scripts must finish within 30 seconds.
*   **Libraries**: Pre-installed set (`ieapp-cli`, `pandas`, `numpy`, `scikit-learn`).
