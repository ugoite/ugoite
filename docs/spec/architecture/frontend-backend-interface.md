# Frontend–Backend Interface (Behavioral Contracts)

This document defines the interaction contracts between the frontend (SolidStart)
and backend (FastAPI). It complements the REST reference by focusing on behavior
and responsibility boundaries.

## Responsibility Matrix

| Feature | Frontend | Backend | Shared Contract |
|---|---|---|---|
| State management | Optimistic updates, local cache, selection/view state | Persistence, history, indexing | `revision_id` optimistic concurrency |
| Validation | UI/form validation, basic format checks | Class validation, business rules, integrity checks | Request/response schemas |
| Search & query | Query construction + display | Indexing, query execution | Query payload shape |
| Code execution | Sandbox UI (future) | Wasm sandbox + MCP host | MCP protocol |

## Interaction Patterns

### Optimistic Updates & Concurrency

- Frontend sends updates with `parent_revision_id`.
- Backend compares `parent_revision_id` with current head.
- On match: backend persists, appends history, returns new `revision_id`.
- On mismatch: backend returns **409 Conflict** with the current revision info.

### Note Creation & Indexing

- Frontend sends Markdown; it does not parse Markdown for business logic.
- Backend/CLI parses frontmatter/H2 sections, updates indices, and returns
  extracted properties (via note list / query / get endpoints).

### Workspace Switching

- Frontend clears selection/editor state on workspace change.
- Frontend reloads workspace-scoped resources (notes, classes, etc.).

## Storage Boundary (Backend ↔ ieapp-cli)

- All filesystem I/O lives in `ieapp-cli` and goes through `fsspec`.
- Backend is a routing/translation layer and must not create directories/files.
- Backend tests must cover `file://` and `memory://` style backends.

## Error Handling Standards

| HTTP | Frontend Behavior | User Feedback |
|---|---|---|
| 400 | Treat as validation bug; log details | "Invalid input" |
| 404 | Remove stale selection; redirect to list | "Not found" |
| 409 | Trigger conflict flow | "Changed on server" |
| 422 | Highlight invalid fields | Field-level error |
| 5xx | Retry/backoff or show offline mode | "Server error" |
