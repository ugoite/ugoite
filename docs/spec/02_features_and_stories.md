# 02. Features & User Stories

## 1. Core User Stories

### Story 1: "The Programmable Knowledge Base" (AI Native)
**As a** power user or AI agent,
**I want** to execute Python code against my notes,
**So that** I can perform complex tasks like "Find all notes mentioning 'Project X', extract the dates, and plot a timeline."

*   **Acceptance Criteria**:
    *   MCP Tool `run_python` is available.
    *   AI can import `ieapp` library in the sandbox.
    *   AI can read/write notes via code.
    *   Output (text/charts) is returned to the AI context.

### Story 2: "Bring Your Own Cloud" (Freedom)
**As a** privacy-conscious user,
**I want** to store my notes on my own S3 bucket or local NAS,
**So that** I have complete control over my data privacy and backup.

*   **Acceptance Criteria**:
    *   Configurable storage backend via connection string (e.g., `s3://my-bucket/notes`).
    *   Seamless switching between local and remote storage.

### Story 3: "The Infinite Canvas" (UI/UX)
**As a** visual thinker,
**I want** to organize my notes on a 2D infinite canvas,
**So that** I can map out complex ideas spatially.

*   **Acceptance Criteria**:
    *   Switch between List and Canvas views.
    *   Drag-and-drop notes.
    *   Visual connections are saved as bi-directional links.

### Story 4: "Time Travel" (Versioning)
**As a** user who made a mistake,
**I want** to view and restore previous versions of a note,
**So that** I never lose important information.

*   **Acceptance Criteria**:
    *   Every save creates a new immutable revision.
    *   UI allows browsing history and reverting.

## 2. Functional Requirements

### FR-01: Storage & Data
*   **FR-01.1**: System MUST use `fsspec` for all I/O.
*   **FR-01.2**: Data MUST be stored in the JSON schema defined in `03_data_model.md`.
*   **FR-01.3**: System MUST support "Append-Only" versioning.

### FR-02: AI & MCP
*   **FR-02.1**: System MUST implement the Model Context Protocol (MCP).
*   **FR-02.2**: System MUST provide a `run_python` tool with a timeout and restricted access (sandbox).
*   **FR-02.3**: System MUST allow the AI to search notes via vector embeddings (FAISS).

### FR-03: Frontend Experience
*   **FR-03.1**: UI MUST be "Local-First" (work offline, sync when online).
*   **FR-03.2**: Editor MUST support Markdown with slash commands.
*   **FR-03.3**: UI MUST support Dark Mode.

## 3. Success Metrics
*   **AI Utility**: The AI can successfully refactor 50 notes in under 30 seconds using code execution.
*   **Performance**: Workspace loads in < 500ms (using cached index).
*   **Reliability**: 99.9% data integrity (verified via HMAC/Checksums).
