# 01. Architecture & Technology Stack

## 1. High-Level Architecture

IEapp follows a **Local-First, Server-Relay** architecture. The backend is stateless and acts as a gateway to the file system (via `fsspec`) and a runtime for AI code execution.

```mermaid
graph TD
    User[User / Browser] -->|HTTP/WebSocket| API[FastAPI Backend]
    AI[AI Agent / Claude] -->|MCP Protocol| API
    
    subgraph "IEapp Backend"
        API -->|Executes| Sandbox[Wasm Sandbox - wasmtime]
        API -->|Reads/Writes| VFS[Virtual File System (fsspec)]
        API -->|Queries| Indexer[Live Indexer (Watch & Parse)]
        Sandbox -->|host.call| API
        Indexer -->|Updates| Cache[Structured Cache (JSON)]
    end
    
    subgraph "Storage Layer"
        VFS -->|Adapter| Local[Local Disk]
        VFS -->|Adapter| S3[S3 / MinIO]
        VFS -->|Adapter| Blob[Azure Blob]
    end
```

## 2. The "Code Execution" Paradigm

Instead of building hundreds of specific tools (e.g., `create_note`, `update_tag`, `calculate_stats`), IEapp exposes a **Wasm Execution Sandbox** to the AI via MCP.

*   **Concept**: The AI is a developer. It interacts with the knowledge base by writing and running JavaScript scripts.
*   **Mechanism**: The backend provides a secure WebAssembly (Wasm) environment (using `wasmtime`) running a JavaScript engine. The AI sends code; the backend runs it in the sandbox. The script can call host functions to interact with the application's REST API.
*   **Benefit**: Infinite flexibility. The AI can perform complex migrations, data analysis, or bulk refactoring without the app developer explicitly building those features.

## 3. The "Structure-from-Text" Engine

To bridge the gap between Markdown freedom and Database structure, IEapp implements a **Live Indexer**.

*   **Input**: Markdown content stored within the system (via API/MCP).
*   **Process**:
    1.  **Detect**: Changes are detected via API write hooks.
    2.  **Parse**: Scans for H2 headers (`## Key`) and extracts the following content as the value.
    3.  **Validate**: Checks against the defined Class (if any) for type safety.
    4.  **Index**: Updates a lightweight `index.json` with the structured data.
*   **Output**: A queryable dataset where "Meeting Notes" become objects with `Date`, `Attendees`, and `Agenda` properties.

## 4. Technology Stack

### Frontend
*   **Runtime**: Bun
*   **Framework**: SolidJS (Solid Start)
*   **Styling**: TailwindCSS
*   **State Management**: Local-first stores (using `solid-js/store` and local storage sync).

### Backend
*   **Runtime**: Python 3.12+
*   **Framework**: FastAPI (ASGI)
*   **Protocol**: HTTP (REST) + MCP over HTTP
*   **Library**: `ieapp` (Core storage and query logic)

### Data & Storage
*   **Abstraction**: `fsspec` (Filesystem Spec)
*   **Format**: JSON (Metadata & Content) + FAISS binary indices (Vector Search)
*   **Search**: FAISS (Vector Search) + In-memory Inverted Index

## 4. Component Responsibilities

### `ieapp` (Library)
*   Core logic for `fsspec` interactions.
*   Implements the "Universal File System" logic.
*   Handles versioning, hashing, and conflict resolution.
*   Provides internal services consumed by the REST API (which the Wasm sandbox accesses via `host.call`).

### `backend` (Service)
*   Hosts the FastAPI server.
*   Implements the MCP Server endpoints.
*   Manages the Wasm Sandbox (security, fuel limits).
*   Handles optional authentication (API keys, bearer tokens) while defaulting to trusted localhost-only mode, and enforces CORS.

### `frontend` (UI)
*   Optimistic UI: Renders changes immediately, syncs in background.
*   Spatial Canvas: Renders the node-link graph.
*   Markdown Editor: Block-based editing experience.
*   **Notebook Renderer**: Renders and executes interactive JavaScript code blocks (via the shared Wasm sandbox).

## 5. Future-Proofing (Experimental)

### BYOAI (Bring Your Own AI) Architecture
Instead of embedding specific AI models, IEapp relies on the **Model Context Protocol (MCP)** to let users bring their own agents.

*   **Voice-to-Schema**: The app provides the raw data (audio/text). The *User's Agent* performs the structuring logic via the MCP `run_script` tool.
*   **Agentic Maintenance**: No hidden background workers. Maintenance tasks (refactoring, tagging) are performed by external agents invoking the MCP `run_script` tool at the user's command.
