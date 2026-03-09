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
- Current entry-editor recovery flow keeps the local draft visible, shows refresh
  guidance, and leaves merge/reconciliation to the user. A dedicated merge UI is
  not part of the current milestone contract.

### Entry Creation & Indexing

- Frontend always sends Markdown to the backend and may compose that Markdown
  from structured inputs (e.g., Web form, Chat), but it does not parse Markdown
  for business logic or indexing rules.
- Frontend supports multiple authoring modes for form-first entries: Markdown,
  Web form, and Chat Q&A. Web form/Chat are transformed to Markdown before API
  submission.
- Backend/CLI parses frontmatter/H2 sections, updates indices, and returns
  extracted properties (via entry list / query / get endpoints).
- For create/update/restore flows, backend MAY pre-check authorization by
  calling `ugoite-core` helpers such as `require_markdown_write()` and
  `require_entry_write()` before invoking the mutation itself.
- ACL evaluation still lives in `ugoite-core`; the backend remains a thin
  adapter that translates HTTP requests into core authorization + mutation
  calls and returns `403 Forbidden` on authorization failure.

### Space Switching

- Frontend clears selection/editor state on space change.
- Frontend reloads space-scoped resources (entries, forms, etc.).

## Storage Boundary (Backend ↔ ugoite-core)

- All runtime filesystem I/O lives behind the shared Rust storage abstraction.
  The current backend/CLI runtime uses `ugoite-core`'s OpenDAL-backed adapter
  layer over `ugoite-minimum`.
- The earlier Python/`fsspec` implementation is historical context only and is
  not part of the current runtime boundary.
- Backend is a routing/translation layer and must not perform direct filesystem operations.
- The fsspec-to-OpenDAL runtime transition is complete. Remaining Milestone 3
  work is about continuing to extract portable logic into `ugoite-minimum`, not
  about maintaining two active runtime storage stacks.
- Backend tests must cover OpenDAL `fs://` (local filesystem) and `memory://`
  backends via the shared core bindings.

## Error Handling Standards

| HTTP | Frontend Behavior | User Feedback |
|---|---|---|
| 400 | Treat as validation bug; log details | "Invalid input" |
| 404 | Remove stale selection; redirect to list | "Not found" |
| 409 | Keep the current draft visible and prompt an explicit refresh | "Changed on server. Refresh to load the latest version." |
| 422 | Highlight invalid fields | Field-level error |
| 5xx | Retry/backoff or show offline mode | "Server error" |
