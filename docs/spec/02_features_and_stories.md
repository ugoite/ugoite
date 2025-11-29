# 02. Features & User Stories

## 1. Core User Stories

### Story 1: "The Programmable Knowledge Base" (AI Native)
**As a** power user or AI agent,
**I want** to execute Python code against my notes,
**So that** I can perform complex tasks like "Find all notes mentioning 'Project X', extract the dates, and plot a timeline."

*   **Acceptance Criteria**:
    *   MCP Tool `run_python_script` is available.
    *   AI can import `ieapp` library in the sandbox.
    *   AI can query structured properties (e.g., `ieapp.query(type="meeting")`).
    *   Output (text/charts) is returned to the AI context.

_Related APIs_: MCP tools `run_python_script`, `search_notes`; REST `POST /workspaces/{ws_id}/query`.

### Story 2: "Structured Freedom" (Data Model)
**As a** user,
**I want** to use standard Markdown headers to define data fields (e.g., `## Date`),
**So that** I can manage structured data with the ease of writing text, without complex forms.

*   **Acceptance Criteria**:
    *   System parses H2 headers as property keys.
    *   Users can define "Classes" (Schemas) to enforce required headers and data types.
    *   Frontend provides validation warnings if a note violates its Class schema.
    *   Creating a note from a Class pre-fills the template.

_Related APIs_: REST `POST /workspaces/{ws_id}/notes`, `PUT /workspaces/{ws_id}/notes/{note_id}`, `GET /workspaces/{ws_id}/schemas`; MCP resource `ieapp://{workspace_id}/schema`.

### Story 3: "Bring Your Own Cloud" (Freedom)
**As a** privacy-conscious user,
**I want** to store my notes on my own S3 bucket or local NAS,
**So that** I have complete control over my data privacy and backup.

*   **Acceptance Criteria**:
    *   Configurable storage backend via connection string (e.g., `s3://my-bucket/notes`).
    *   Seamless switching between local and remote storage.

_Related APIs_: REST `PATCH /workspaces/{id}` for storage connectors and `POST /workspaces/{id}/test-connection` for validation.

### Story 4: "The Infinite Canvas" (UI/UX)
**As a** visual thinker,
**I want** to organize my notes on a 2D infinite canvas,
**So that** I can map out complex ideas spatially.

*   **Acceptance Criteria**:
    *   Switch between List and Canvas views.
    *   Drag-and-drop notes.
    *   Visual connections are saved as bi-directional links.

_Related APIs_: REST `PUT /workspaces/{ws_id}/notes/{note_id}` (persists `canvas_position`) and `POST /workspaces/{ws_id}/links` / `DELETE /workspaces/{ws_id}/links/{link_id}` for edges.

### Story 5: "Time Travel" (Versioning)
**As a** user who made a mistake,
**I want** to view and restore previous versions of a note,
**So that** I never lose important information.

*   **Acceptance Criteria**:
    *   Every save creates a new immutable revision.
    *   UI allows browsing history and reverting.

_Related APIs_: REST `GET /workspaces/{ws_id}/notes/{note_id}/history`, `GET /workspaces/{ws_id}/notes/{note_id}/history/{revision_id}`, and `POST /workspaces/{ws_id}/notes/{note_id}/restore`; MCP tool `notes.restore` for agent-driven reverts.

## 2. Advanced / Experimental Features (Trendy & Bold)

### Story 6: "Voice-to-Schema" (BYOAI - Bring Your Own AI)
**As a** mobile user,
**I want** to record a voice memo and have my chosen AI assistant structure it,
**So that** I can use the best AI model available (Claude, GPT-4, etc.) without being locked into the app's built-in model.

*   **Acceptance Criteria**:
    *   User records audio; app saves it as a note attachment.
    *   User invokes their connected MCP Agent (e.g., "Format this voice note").
    *   Agent reads the transcript (via MCP), extracts fields, and updates the note with H2 headers.

_Related APIs_: REST `POST /workspaces/{ws_id}/attachments`, `POST /workspaces/{ws_id}/notes/{note_id}/attachments`, and MCP resources `ieapp://{workspace_id}/notes/{note_id}` for transcript access followed by `run_python_script` updates.

### Story 7: "Computational Notebooks" (Live Code)
**As a** data-driven user,
**I want** to embed Python code blocks in my notes that execute and render results,
**So that** I can create live reports or analyze my own knowledge base within the app.

*   **Acceptance Criteria**:
    *   Markdown code blocks tagged `python` can be executed.
    *   Code runs in the same sandbox as the MCP agent.
    *   Output (text, tables, charts) is rendered interactively below the code block.

_Related APIs_: REST `POST /workspaces/{ws_id}/notes/{note_id}/blocks/{block_id}/execute` (frontend trigger) and MCP tool `run_python_script` (shared sandbox runtime).

### Story 8: "Agentic Refactoring" (BYOAI)
**As a** user with a messy workspace,
**I want** to ask my AI agent to "clean up this folder",
**So that** I can leverage external intelligence to organize my knowledge base on demand.

*   **Acceptance Criteria**:
    *   App exposes `list_notes`, `read_note`, and `update_note` tools via MCP.
    *   User asks Agent: "Find duplicates in Project X and merge them."
    *   Agent executes the logic (potentially using `run_python_script` for batch processing) and presents the result for confirmation.

_Related APIs_: MCP tools `notes.list`, `notes.read`, `notes.update`, `notes.delete`, plus `run_python_script`; REST `GET/PUT /workspaces/{ws_id}/notes/{note_id}` for confirmation flows.

## 3. Functional Requirements

### FR-01: Storage & Data
*   **FR-01.1**: System MUST use `fsspec` for all I/O.
*   **FR-01.2**: Data MUST be stored in the JSON schema defined in `03_data_model.md`.
*   **FR-01.3**: System MUST support "Append-Only" versioning.
*   **FR-01.4**: System MUST parse Markdown to extract Frontmatter and H2 Headers (`## Key`) into a structured index.

### FR-02: AI & MCP
*   **FR-02.1**: System MUST implement the Model Context Protocol (MCP).
*   **FR-02.2**: System MUST provide a `run_python_script` tool with a timeout and restricted access (sandbox).
*   **FR-02.3**: System MUST allow the AI to search notes via vector embeddings (FAISS) and structured queries.

### FR-03: Frontend Experience
*   **FR-03.1**: UI MUST be "Local-First" (work offline, sync when online).
*   **FR-03.2**: Editor MUST support Markdown with slash commands.
*   **FR-03.3**: UI MUST support Dark Mode.

## 4. Success Metrics
*   **AI Utility**: The AI can successfully refactor 50 notes in under 30 seconds using code execution.
*   **Performance**: Workspace loads in < 500ms (using cached index).
*   **Reliability**: 99.9% data integrity (verified via HMAC/Checksums).
