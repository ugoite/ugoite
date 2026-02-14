# Frontend–Backend Interface (Behavioral Contracts)

This document defines the interaction contracts between the frontend (SolidStart)
and backend (FastAPI). It complements the REST reference by focusing on behavior
and responsibility boundaries.

## Responsibility Matrix

| Feature | Frontend | Backend | Shared Contract |
|---|---|---|---|
| State management | Optimistic updates, local cache, selection/view state | Persistence, history, indexing | `revision_id` optimistic concurrency |
| Validation | UI/form validation, basic format checks | Form validation, business rules, integrity checks | Request/response models |
| Search & query | Query construction + display | Indexing, query execution | Query payload shape |
| Code execution | Code execution UI (future) | MCP host | MCP protocol |

## Interaction Patterns

### Optimistic Updates & Concurrency

- Frontend sends updates with `parent_revision_id`.
- Backend compares `parent_revision_id` with current head.
- On match: backend persists, appends history, returns new `revision_id`.
- On mismatch: backend returns **409 Conflict** with the current revision info.

### Entry Creation & Indexing

- Frontend sends Markdown; it does not parse Markdown for business logic.
- Frontend supports multiple authoring modes for form-first entries: Markdown,
  Web form, and Chat Q&A. Web form/Chat are transformed to Markdown before API
  submission.
- Backend/CLI parses frontmatter/H2 sections, updates indices, and returns
  extracted properties (via entry list / query / get endpoints).

### Space Switching

- Frontend clears selection/editor state on space change.
- Frontend reloads space-scoped resources (entries, forms, etc.).

## Storage Boundary (Backend ↔ ugoite-core)

- All filesystem I/O lives in `ugoite-core` (currently via `fsspec`, transitioning to OpenDAL).
- Backend is a routing/translation layer and must not perform direct filesystem operations.
- Backend tests must cover `fs://` and `memory://` style backends via `ugoite-core`.

## Error Handling Standards

| HTTP | Frontend Behavior | User Feedback |
|---|---|---|
| 400 | Treat as validation bug; log details | "Invalid input" |
| 404 | Remove stale selection; redirect to list | "Not found" |
| 409 | Trigger conflict flow | "Changed on server" |
| 422 | Highlight invalid fields | Field-level error |
| 5xx | Retry/backoff or show offline mode | "Server error" |
