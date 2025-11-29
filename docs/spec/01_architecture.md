# 01. Architecture & Technology Stack

## 1. High-Level Architecture

IEapp follows a **Local-First, Server-Relay** architecture. The backend is stateless and acts as a gateway to the file system (via `fsspec`) and a runtime for AI code execution.

```mermaid
graph TD
    User[User / Browser] -->|HTTP/WebSocket| API[FastAPI Backend]
    AI[AI Agent / Claude] -->|MCP Protocol| API
    
    subgraph "IEapp Backend"
        API -->|Executes| Sandbox[Python Code Sandbox]
        API -->|Reads/Writes| VFS[Virtual File System (fsspec)]
        API -->|Queries| Indexer[Live Indexer (Watch & Parse)]
        Sandbox -->|Access| VFS
        Indexer -->|Updates| Cache[Structured Cache (JSON)]
    end
    
    subgraph "Storage Layer"
        VFS -->|Adapter| Local[Local Disk]
        VFS -->|Adapter| S3[S3 / MinIO]
        VFS -->|Adapter| Blob[Azure Blob]
    end
```

## 2. The "Code Execution" Paradigm (New for v2)

Instead of building hundreds of specific tools (e.g., `create_note`, `update_tag`, `calculate_stats`), IEapp exposes a **Python Execution Sandbox** to the AI via MCP.

*   **Concept**: The AI is a developer. It interacts with the knowledge base by writing and running Python scripts.
*   **Mechanism**: The backend provides a pre-configured Python environment with `ieapp-cli` installed. The AI sends code; the backend runs it in a secure sandbox and returns the stdout/stderr/result.
*   **Benefit**: Infinite flexibility. The AI can perform complex migrations, data analysis, or bulk refactoring without the app developer explicitly building those features.

## 3. The "Structure-from-Text" Engine

To bridge the gap between Markdown freedom and Database structure, IEapp implements a **Live Indexer**.

*   **Input**: Standard Markdown files with H2 Headers (e.g., `## Date`).
*   **Process**:
    1.  **Watch**: Detects file changes via `fsspec` or polling.
    2.  **Parse**: Scans for H2 headers (`## Key`) and extracts the following content as the value.
    3.  **Validate**: Checks against the defined "Class Schema" (if any) for type safety.
    4.  **Index**: Updates a lightweight `index.json` with the structured data.
*   **Output**: A queryable dataset where "Meeting Notes" become objects with `Date`, `Attendees`, and `Agenda` properties.

## 4. Technology Stack

### Frontend
*   **Runtime**: Bun
*   **Framework**: SolidJS (Solid Start)
*   **Styling**: TailwindCSS
*   **State Management**: Local-first stores (using `solid-js/store` and local storage sync).

### Backend
*   **Runtime**: Python 3.13+
*   **Framework**: FastAPI (ASGI)
*   **Protocol**: HTTP/2 (REST) + SSE (Server-Sent Events for MCP)
*   **CLI Library**: Typer (for `ieapp-cli`)

### Data & Storage
*   **Abstraction**: `fsspec` (Filesystem Spec)
*   **Format**: JSON (Metadata & Content) + Parquet (Vector Indices)
*   **Search**: FAISS (Vector Search) + In-memory Inverted Index

## 4. Component Responsibilities

### `ieapp-cli` (Library)
*   Core logic for `fsspec` interactions.
*   Implements the "Universal File System" logic.
*   Handles versioning, hashing, and conflict resolution.
*   Exposes the Python API that the AI will use within the sandbox.

### `backend` (Service)
*   Hosts the FastAPI server.
*   Implements the MCP Server endpoints.
*   Manages the Python Code Sandbox (security, timeouts).
*   Handles Authentication (if enabled) and CORS.

### `frontend` (UI)
*   Optimistic UI: Renders changes immediately, syncs in background.
*   Spatial Canvas: Renders the node-link graph.
*   Markdown Editor: Block-based editing experience.
*   **Notebook Renderer**: Renders and executes interactive Python code blocks.

## 5. Future-Proofing (Experimental)

### BYOAI (Bring Your Own AI) Architecture
Instead of embedding specific AI models, IEapp relies on the **Model Context Protocol (MCP)** to let users bring their own agents.

*   **Voice-to-Schema**: The app provides the raw data (audio/text). The *User's Agent* performs the structuring logic via MCP tools.
*   **Agentic Maintenance**: No hidden background workers. Maintenance tasks (refactoring, tagging) are performed by external agents invoking MCP tools (`search_notes`, `update_note`) at the user's command.
