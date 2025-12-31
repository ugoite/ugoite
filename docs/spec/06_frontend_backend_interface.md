# Frontend-Backend Interface & Responsibility Boundaries

## Overview
This document defines the architectural boundaries and interaction patterns between the IEapp Frontend (SolidStart) and Backend (FastAPI). It complements the API specification (04_api_and_mcp.md) by focusing on *behavioral contracts* and *responsibility separation*.

## Responsibility Matrix

| Feature | Frontend Responsibility | Backend Responsibility | Shared Contract |
| :--- | :--- | :--- | :--- |
| **State Management** | Optimistic updates, local cache (Store), UI state (selection, view mode). | Authoritative state persistence, history tracking, indexing. | `revision_id` for concurrency control. |
| **Data Validation** | Form validation (required fields), basic format checks. | Schema enforcement, business rule validation, integrity checks. | JSON Schema / Pydantic Models. |
| **Search & Query** | Query construction, result display, highlighting. | Indexing, query execution, ranking, filtering. | Query DSL (JSON-based filter). |
| **Code Execution** | Sandbox UI, input collection, output rendering. | Wasm Sandbox isolation, resource limits, security enforcement. | MCP Protocol (Tool execution). |

## Interaction Patterns

### 1. Optimistic Updates & Concurrency
To ensure a responsive UI, the frontend implements optimistic updates for note modifications.

- **Frontend**:
  1.  User edits note.
  2.  Store immediately updates local state.
  3.  Store sends `PUT /notes/{id}` with `parent_revision_id`.
  4.  **Success**: Update confirmed, `revision_id` updated.
  5.  **Failure (409 Conflict)**:
      -   Store rolls back local change.
      -   UI displays conflict warning.
      -   Store fetches latest server state.
      -   User resolves conflict manually (or last-write-wins if implemented).

- **Backend**:
  1.  Receives update request.
  2.  Checks `parent_revision_id` against current head.
  3.  **Match**: Applies update, appends to history, returns 200 OK with new `revision_id`.
  4.  **Mismatch**: Returns 409 Conflict with `current_revision_id`.

### 2. Note Creation & Indexing
- **Frontend**:
  -   Sends Markdown content.
  -   Does *not* parse metadata (frontmatter/headers) for business logic.
  -   Relies on Backend to return parsed properties.
- **Backend**:
  -   Receives Markdown.
  -   Parses headers/frontmatter.
  -   Updates Search Index.
  -   Returns created note with extracted properties.

### 3. Canvas Placeholder (Milestone 5)
- **Frontend**:
  -   Renders static grid layout.
  -   Calculates positions deterministically based on list index (temporary).
  -   No persistence of positions yet.
- **Backend**:
  -   Stores `canvas_position` in note metadata (prepared for future).
  -   Currently ignores position updates if sent, or persists them blindly.

## Error Handling Standards

| HTTP Status | Frontend Behavior | User Feedback |
| :--- | :--- | :--- |
| **400 Bad Request** | Log error, check validation logic. | "Invalid input" toast. |
| **401/403 Auth** | Redirect to login (future) or show config error. | "Session expired" or "Access denied". |
| **404 Not Found** | Remove from local list, redirect to list view. | "Note not found". |
| **409 Conflict** | Trigger conflict resolution flow. | "Content has changed on server". |
| **422 Validation** | Highlight invalid fields. | Field-specific error messages. |
| **5xx Server Error** | Retry (exponential backoff) or show offline mode. | "Server error, retrying...". |

## Workspace Management

### Initialization Flow
On application startup, the frontend performs workspace initialization:

1. **Load workspaces**: Fetch list of existing workspaces from backend.
2. **Restore selection**: Check localStorage for previously selected workspace.
3. **Default workspace**: If no workspaces exist, automatically create a "default" workspace.
4. **Persist selection**: Store selected workspace ID in localStorage for session continuity.

### Workspace Selector
- **UI Location**: Top of the sidebar, above the note list.
- **Features**:
  - Dropdown for quick workspace switching.
  - "+" button to create new workspaces.
  - Persisted selection across sessions.
- **State reset**: When switching workspaces, clear editor state and reload notes.

## Linting & Code Quality
- **Frontend**: Strict `Biome` configuration ensures no leaked backend logic (e.g., direct DB access, loose typing).
- **Backend**: `Ruff` and `Mypy` ensure type safety and schema compliance.
