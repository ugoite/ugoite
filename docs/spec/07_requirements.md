# 07. Requirements & Test Mapping

This document systematically organizes IEapp requirements and specifies which tests verify each requirement.

## Legend

| Test Type | Tool | Location |
|-----------|------|----------|
| **pytest** | pytest (Python) | `backend/tests/`, `ieapp-cli/tests/` |
| **vitest** | vitest (TypeScript) | `frontend/src/**/*.test.ts(x)` |
| **e2e** | bun:test (TypeScript) | `e2e/` |

---

## 1. Storage & Data Model Requirements

### REQ-STO-001: Storage Abstraction Using fsspec
**Related Spec**: [01_architecture.md](01_architecture.md) §1, [02_features_and_stories.md](02_features_and_stories.md) FR-01.1

System MUST use `fsspec` for all I/O operations.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_workspace.py` | `test_create_workspace_scaffolding` |
| pytest | `ieapp-cli/tests/test_workspace.py` | `test_create_workspace_s3_unimplemented` |
| pytest | `ieapp-cli/tests/test_indexer.py` | `test_indexer_run_once` (memory fs) |

---

### REQ-STO-002: Workspace Directory Structure
**Related Spec**: [03_data_model.md](03_data_model.md) §2

Workspace has the following structure:
- `meta.json`, `settings.json`
- `schemas/`, `index/`, `attachments/`, `notes/`
- `index/index.json`, `index/stats.json`

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_workspace.py` | `test_create_workspace_scaffolding` |
| pytest | `backend/tests/test_api.py` | `test_create_workspace` |

---

### REQ-STO-003: File Permissions (chmod 600)
**Related Spec**: [05_security_and_quality.md](05_security_and_quality.md) §1

Apply chmod 600 to data directories to ensure privacy.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_workspace.py` | `test_create_workspace_scaffolding` (mode assertion) |

---

### REQ-STO-004: Workspace Management via global.json
**Related Spec**: [03_data_model.md](03_data_model.md) §6.1

`global.json` maintains workspace registry and HMAC keys.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_workspace.py` | `test_create_workspace_scaffolding` |
| pytest | `ieapp-cli/tests/test_integrity.py` | `test_integrity_provider_for_workspace_success` |
| pytest | `ieapp-cli/tests/test_integrity.py` | `test_integrity_provider_missing_hmac_key` |

---

### REQ-STO-005: Prevent Duplicate Workspace Creation
**Related Spec**: [02_features_and_stories.md](02_features_and_stories.md) FR-01.x

Returns 409 error when attempting to create a workspace with an existing ID.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_workspace.py` | `test_create_workspace_idempotency` |
| pytest | `backend/tests/test_api.py` | `test_create_workspace_conflict` |
| vitest | `frontend/src/lib/client.test.ts` | `should throw error for duplicate workspace` |

---

### REQ-STO-006: Storage Connector Validation
**Related Spec**: [02_features_and_stories.md](02_features_and_stories.md) Story 3, [04_api_and_mcp.md](04_api_and_mcp.md) §1 Workspaces

Workspaces MUST accept storage connector updates and provide a validation endpoint before committing changes.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `backend/tests/test_api.py` | `test_update_workspace_storage_connector` |
| pytest | `backend/tests/test_api.py` | `test_test_connection_endpoint` |

---

## 2. Note Management Requirements

### REQ-NOTE-001: Note Creation
**Related Spec**: [04_api_and_mcp.md](04_api_and_mcp.md) §1 Notes

Create note from Markdown content, generating `meta.json`, `content.json`, `history/index.json`.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_notes.py` | `test_create_note_basic` |
| pytest | `backend/tests/test_api.py` | `test_create_note` |
| pytest | `backend/tests/test_api.py` | `test_create_note_conflict` |
| vitest | `frontend/src/lib/client.test.ts` | `should create a note and extract title from markdown` |
| vitest | `frontend/src/lib/store.test.ts` | `should create a note and reload list` |
| e2e | `e2e/notes.test.ts` | `POST /workspaces/default/notes creates a new note` |

---

### REQ-NOTE-002: Optimistic Concurrency via Revisions
**Related Spec**: [06_frontend_backend_interface.md](06_frontend_backend_interface.md) §Interaction Patterns

Validate `parent_revision_id` during update; return 409 Conflict on mismatch.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_notes.py` | `test_update_note_revision_mismatch` |
| pytest | `backend/tests/test_api.py` | `test_update_note_conflict` |
| vitest | `frontend/src/lib/client.test.ts` | `should throw RevisionConflictError (409) on revision mismatch` |

---

### REQ-NOTE-003: Note Update
**Related Spec**: [04_api_and_mcp.md](04_api_and_mcp.md) §1 Notes

Update note and create new revision.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_notes.py` | `test_note_history_append` |
| pytest | `ieapp-cli/tests/test_notes.py` | `test_note_history_diff` |
| pytest | `backend/tests/test_api.py` | `test_update_note` |
| vitest | `frontend/src/lib/client.test.ts` | `should update note with correct parent_revision_id` |
| vitest | `frontend/src/lib/store.test.ts` | `should apply optimistic updates during update` |
| e2e | `e2e/notes.test.ts` | `PUT /workspaces/default/notes/:id updates note` |

---

### REQ-NOTE-004: Note Deletion (Tombstone)
**Related Spec**: [04_api_and_mcp.md](04_api_and_mcp.md) §1 Notes

Delete note (tombstone) and exclude from list.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `backend/tests/test_api.py` | `test_delete_note` |
| vitest | `frontend/src/lib/client.test.ts` | `should remove note from list` |
| vitest | `frontend/src/lib/store.test.ts` | `should delete note optimistically` |
| e2e | `e2e/notes.test.ts` | `DELETE /workspaces/default/notes/:id removes note` |

---

### REQ-NOTE-005: Note History (Time Travel)
**Related Spec**: [02_features_and_stories.md](02_features_and_stories.md) Story 5, [04_api_and_mcp.md](04_api_and_mcp.md) §1 Notes

Every save creates an immutable revision; history can be browsed and restored.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_notes.py` | `test_note_history_append` |
| pytest | `ieapp-cli/tests/test_notes.py` | `test_note_history_diff` |
| pytest | `backend/tests/test_api.py` | `test_get_note_history` |
| pytest | `backend/tests/test_api.py` | `test_get_note_revision` |
| pytest | `backend/tests/test_api.py` | `test_restore_note` |

---

### REQ-NOTE-006: Structured Data Extraction from Markdown
**Related Spec**: [02_features_and_stories.md](02_features_and_stories.md) Story 2, [03_data_model.md](03_data_model.md) §3

Extract H2 headers (`## Key`) as fields and merge with frontmatter.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_notes.py` | `test_markdown_sections_persist` |
| pytest | `ieapp-cli/tests/test_indexer.py` | `test_extract_properties_h2_sections` |
| pytest | `ieapp-cli/tests/test_indexer.py` | `test_extract_properties_precedence` |
| vitest | `frontend/src/lib/client.test.ts` | `should extract H2 headers as properties` |

---

### REQ-NOTE-007: Properties and Links in List Response
**Related Spec**: [04_api_and_mcp.md](04_api_and_mcp.md) §1 Notes

Include `properties` and `links` fields when retrieving note list.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_notes.py` | `test_list_notes_returns_properties_and_links` |
| pytest | `backend/tests/test_api.py` | `test_list_notes` |

---

### REQ-NOTE-008: Attachments Upload & Linking
**Related Spec**: [02_features_and_stories.md](02_features_and_stories.md) Story 6, [04_api_and_mcp.md](04_api_and_mcp.md) §1 Attachments

Upload binary attachments, return a generated attachment ID, and allow notes to reference attachments in `content.json`.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `backend/tests/test_api.py` | `test_upload_attachment_and_link_to_note` |

---

### REQ-NOTE-009: Attachment Garbage Collection Guard
**Related Spec**: [03_data_model.md](03_data_model.md) §2 attachments

Prevent deleting attachments that are still referenced by any note.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `backend/tests/test_api.py` | `test_delete_attachment_referenced_fails` |

---

### REQ-NOTE-010: Canvas Links CRUD
**Related Spec**: [02_features_and_stories.md](02_features_and_stories.md) Story 4, [04_api_and_mcp.md](04_api_and_mcp.md) §1 Canvas Links

Create and delete bi-directional links between notes; list links across workspace.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `backend/tests/test_api.py` | `test_create_and_list_links` |
| pytest | `backend/tests/test_api.py` | `test_delete_link_updates_notes` |

---

## 3. Indexer Requirements

### REQ-IDX-001: Structured Cache via Live Indexer
**Related Spec**: [01_architecture.md](01_architecture.md) §3, [03_data_model.md](03_data_model.md) §5

Update `index/index.json` and `index/stats.json` on note changes.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_indexer.py` | `test_indexer_run_once` |
| pytest | `ieapp-cli/tests/test_indexer.py` | `test_aggregate_stats` |
| pytest | `ieapp-cli/tests/test_indexer.py` | `test_aggregate_stats_includes_field_usage` |

---

### REQ-IDX-002: Schema Validation
**Related Spec**: [03_data_model.md](03_data_model.md) §4

Validate properties against class definitions and generate warnings.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_indexer.py` | `test_validate_properties_missing_required` |
| pytest | `ieapp-cli/tests/test_indexer.py` | `test_validate_properties_valid` |
| pytest | `ieapp-cli/tests/test_indexer.py` | `test_validate_properties_casting` |
| pytest | `ieapp-cli/tests/test_indexer.py` | `test_validate_properties_invalid_type` |

---

### REQ-IDX-003: Structured Queries
**Related Spec**: [04_api_and_mcp.md](04_api_and_mcp.md) §1 Query

Execute structured queries against the index.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_indexer.py` | `test_query_index` |
| pytest | `ieapp-cli/tests/test_indexer.py` | `test_query_index_by_tag` |
| pytest | `backend/tests/test_api.py` | `test_query_notes` |

---

### REQ-IDX-004: Inverted Index Generation
**Related Spec**: [03_data_model.md](03_data_model.md) §5

Generate inverted index for keyword search.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_indexer.py` | `test_indexer_generates_inverted_index` |

---

### REQ-IDX-005: Word Count Calculation
**Related Spec**: [03_data_model.md](03_data_model.md) §3 Auto Properties

Calculate word count as an automatic property.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_indexer.py` | `test_indexer_computes_word_count` |

---

### REQ-IDX-006: Indexing via Watch Loop
**Related Spec**: [01_architecture.md](01_architecture.md) §3

Trigger indexer in response to filesystem events.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_indexer.py` | `test_indexer_watch_loop_triggers_run` |

---

### REQ-IDX-007: Hybrid Search Endpoint
**Related Spec**: [02_features_and_stories.md](02_features_and_stories.md) Story 3, [04_api_and_mcp.md](04_api_and_mcp.md) §1 Search

Expose `GET /workspaces/{ws_id}/search` using the inverted index (keyword) and falling back to a simple scan when the index is absent.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `backend/tests/test_api.py` | `test_search_returns_matches` |

---

## 4. Integrity Requirements

### REQ-INT-001: HMAC Signature
**Related Spec**: [05_security_and_quality.md](05_security_and_quality.md) §1

Sign all data revisions with locally generated key to prevent tampering.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_integrity.py` | `test_integrity_provider_for_workspace_success` |
| pytest | `ieapp-cli/tests/test_integrity.py` | `test_integrity_provider_missing_hmac_key` |
| pytest | `ieapp-cli/tests/test_integrity.py` | `test_integrity_provider_invalid_hmac_key` |
| pytest | `ieapp-cli/tests/test_notes.py` | `test_note_history_append` (checksum/signature) |

---

### REQ-INT-002: Checksum Verification
**Related Spec**: [03_data_model.md](03_data_model.md) §6.3

Calculate and store checksums for note content.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_notes.py` | `test_note_history_append` |

---

### REQ-INT-003: HMAC Signature for API Responses
**Related Spec**: [05_security_and_quality.md](05_security_and_quality.md) §1

Add HMAC signature header to API responses.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `backend/tests/test_api.py` | `test_middleware_hmac_signature` |

---

## 5. Security Requirements

### REQ-SEC-001: Localhost Binding
**Related Spec**: [05_security_and_quality.md](05_security_and_quality.md) §1

API binds to `127.0.0.1` only by default.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `backend/tests/test_api.py` | `test_middleware_blocks_remote_clients` |

---

### REQ-SEC-002: Security Headers
**Related Spec**: [05_security_and_quality.md](05_security_and_quality.md) §1

Set security-related HTTP headers.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `backend/tests/test_api.py` | `test_middleware_headers` |

---

## 6. Sandbox (Code Execution) Requirements

### REQ-SANDBOX-001: JavaScript Execution
**Related Spec**: [01_architecture.md](01_architecture.md) §2, [04_api_and_mcp.md](04_api_and_mcp.md) §2 run_script

Execute JavaScript code within Wasm sandbox.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_sandbox.py` | `test_simple_execution` |
| pytest | `backend/tests/test_sandbox.py` | `test_simple_execution` |

---

### REQ-SANDBOX-002: API Calls via host.call
**Related Spec**: [04_api_and_mcp.md](04_api_and_mcp.md) §2 run_script

Call APIs from within sandbox using `host.call`.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_sandbox.py` | `test_host_call` |
| pytest | `backend/tests/test_sandbox.py` | `test_host_call` |

---

### REQ-SANDBOX-003: Execution Error Handling
**Related Spec**: [05_security_and_quality.md](05_security_and_quality.md) §1 Code Sandbox Security

Properly capture and report script errors.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_sandbox.py` | `test_execution_error` |
| pytest | `backend/tests/test_sandbox.py` | `test_execution_error` |

---

### REQ-SANDBOX-004: Prevent Infinite Loops via Fuel Limits
**Related Spec**: [05_security_and_quality.md](05_security_and_quality.md) §1 Code Sandbox Security

Prevent infinite loops through fuel (CPU cycles) limits.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_sandbox.py` | `test_infinite_loop_fuel` |
| pytest | `backend/tests/test_sandbox.py` | `test_infinite_loop_fuel` |

---

### REQ-SANDBOX-005: Wasm Artifact Validation
**Related Spec**: [05_security_and_quality.md](05_security_and_quality.md) §1

Raise appropriate error when Wasm file is missing.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `ieapp-cli/tests/test_sandbox.py` | `test_missing_wasm_raises` |
| pytest | `backend/tests/test_sandbox.py` | `test_missing_wasm_raises` |

---

## 7. REST API Requirements

### REQ-API-001: Workspace CRUD
**Related Spec**: [04_api_and_mcp.md](04_api_and_mcp.md) §1 Workspaces

Create, retrieve, and list workspaces.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `backend/tests/test_api.py` | `test_create_workspace` |
| pytest | `backend/tests/test_api.py` | `test_list_workspaces` |
| pytest | `backend/tests/test_api.py` | `test_get_workspace` |
| pytest | `backend/tests/test_api.py` | `test_get_workspace_not_found` |
| vitest | `frontend/src/lib/client.test.ts` | `workspaceApi.list` |
| vitest | `frontend/src/lib/client.test.ts` | `workspaceApi.create` |
| e2e | `e2e/smoke.test.ts` | `GET /workspaces returns list` |
| e2e | `e2e/smoke.test.ts` | `GET /workspaces includes default workspace` |

---

### REQ-API-002: Note CRUD
**Related Spec**: [04_api_and_mcp.md](04_api_and_mcp.md) §1 Notes

Create, retrieve, update, and delete notes.

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `backend/tests/test_api.py` | `test_create_note`, `test_list_notes`, `test_get_note`, `test_update_note`, `test_delete_note` |
| vitest | `frontend/src/lib/client.test.ts` | `noteApi.list`, `noteApi.create`, `noteApi.get`, `noteApi.update`, `noteApi.delete` |
| e2e | `e2e/notes.test.ts` | Create/Read/Update/Delete Notes |

---

### REQ-API-003: Health Check
**Related Spec**: None (Infrastructure requirement)

Check server health via `/health` endpoint.

| Test Type | File | Test Name |
|-----------|------|-----------|
| e2e | `e2e/smoke.test.ts` | `GET /health returns OK` |

---

## 8. Frontend Requirements

### REQ-FE-001: Workspace Selector
**Related Spec**: [06_frontend_backend_interface.md](06_frontend_backend_interface.md) §Workspace Management

Select, switch, and create workspaces.

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/components/WorkspaceSelector.test.tsx` | 全テスト |
| vitest | `frontend/src/lib/workspace-store.test.ts` | 全テスト |

---

### REQ-FE-002: Automatic Default Workspace Creation
**Related Spec**: [06_frontend_backend_interface.md](06_frontend_backend_interface.md) §Workspace Management

Automatically create "default" workspace when none exist.

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/lib/workspace-store.test.ts` | `should create default workspace when none exist` |

---

### REQ-FE-003: Persist Workspace Selection
**Related Spec**: [06_frontend_backend_interface.md](06_frontend_backend_interface.md) §Workspace Management

Save selected workspace to localStorage and maintain across sessions.

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/lib/workspace-store.test.ts` | `should restore persisted workspace selection` |
| vitest | `frontend/src/lib/workspace-store.test.ts` | `should select workspace and persist choice` |

---

### REQ-FE-004: Note List Display
**Related Spec**: [02_features_and_stories.md](02_features_and_stories.md) FR-03.x

Display note list with titles and properties.

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/components/NoteList.test.tsx` | 全テスト |
| vitest | `frontend/src/lib/store.test.ts` | `should load notes from API` |

---

### REQ-FE-005: Markdown Editor
**Related Spec**: [02_features_and_stories.md](02_features_and_stories.md) FR-03.2

Editor with Markdown editing, preview, and save functionality.

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/components/MarkdownEditor.test.tsx` | 全テスト |

---

### REQ-FE-005a: Editor Content Graceful Handling
**Related Spec**: [06_frontend_backend_interface.md](06_frontend_backend_interface.md) §1 Data Binding

Editor MUST handle undefined/null content gracefully, displaying empty string instead of "undefined" text.
New note creation MUST display initial content immediately without waiting for resource load.
Preview mode MUST NOT crash when content is undefined.

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/components/MarkdownEditor.test.tsx` | `should handle undefined content gracefully` |
| vitest | `frontend/src/components/MarkdownEditor.test.tsx` | `should handle null content gracefully` |
| vitest | `frontend/src/components/MarkdownEditor.test.tsx` | `should render preview with undefined content without crashing` |
| vitest | `frontend/src/components/MarkdownEditor.test.tsx` | `should display placeholder when content is empty` |

---

### REQ-FE-006: Optimistic Updates
**Related Spec**: [06_frontend_backend_interface.md](06_frontend_backend_interface.md) §1 Optimistic Updates

UI reflects changes immediately and syncs in background.

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/lib/store.test.ts` | `should apply optimistic updates during update` |
| vitest | `frontend/src/lib/store.test.ts` | `should delete note optimistically` |

---

### REQ-FE-007: Canvas Placeholder
**Related Spec**: [02_features_and_stories.md](02_features_and_stories.md) Story 4

Placeholder for 2D canvas view (full implementation planned for Milestone 6).

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/components/CanvasPlaceholder.test.tsx` | 全テスト |

---

### REQ-FE-008: Note Selection and Highlight
**Related Spec**: [06_frontend_backend_interface.md](06_frontend_backend_interface.md) §Feature Matrix

Highlight selected note and load details.

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/components/NoteList.test.tsx` | `should highlight selected note` |
| vitest | `frontend/src/components/CanvasPlaceholder.test.tsx` | `should highlight selected note` |
| vitest | `frontend/src/lib/store.test.ts` | `should handle note selection` |

---

### REQ-FE-009: Conflict Message Display
**Related Spec**: [06_frontend_backend_interface.md](06_frontend_backend_interface.md) §Error Handling

Notify user on 409 Conflict error.

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/components/MarkdownEditor.test.tsx` | `should show conflict message when there is a conflict` |

---

### REQ-FE-010: Editor Content Persistence During Save
**Related Spec**: [06_frontend_backend_interface.md](06_frontend_backend_interface.md) §Interaction Patterns

Editor content MUST NOT be overwritten during or after save operation.
Save operation MUST NOT trigger content reload from server.
Consecutive saves MUST work correctly by tracking revision_id locally after each save.

**Critical Implementation Requirements:**
1. `store.updateNote()` MUST NOT call `refetchSelectedNote()` after successful save
2. `handleSave()` MUST update local `currentRevisionId` with server response's `revision_id`
3. `createEffect` for content sync MUST only run when note ID changes, not on every resource update

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/lib/store.test.ts` | `should not refetch after successful update` |
| vitest | `frontend/src/lib/store.test.ts` | `should preserve editor content during save` |
| e2e | `e2e/notes.test.ts` | `saved content should persist after reload (REQ-FE-010)` |

---

### REQ-FE-011: Editor Content Sync on Note Switch Only
**Related Spec**: [06_frontend_backend_interface.md](06_frontend_backend_interface.md) §Interaction Patterns

Editor content synchronization with server MUST only occur when:
1. User explicitly selects a different note
2. User explicitly clicks refresh button
3. Workspace is changed

Editor content MUST NOT be overwritten when:
1. Save operation completes
2. Selected note resource is refetched
3. Note list is reloaded

**Critical Implementation Requirements:**
1. Track `lastLoadedNoteId` to detect note switches vs resource updates
2. Only update `editorContent` when `note.id !== lastLoadedNoteId`
3. Reset `lastLoadedNoteId` when workspace changes

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/lib/store.test.ts` | `should only sync content on note switch` |
| vitest | `frontend/src/lib/store.test.ts` | `should not overwrite content on refetch` |

---

### REQ-FE-012: Consecutive Save Support
**Related Spec**: [06_frontend_backend_interface.md](06_frontend_backend_interface.md) §Interaction Patterns

Multiple consecutive saves on the same note MUST work correctly.
After each successful save, the local revision_id MUST be updated to enable the next save.
Failing to update revision_id would cause 409 Conflict on subsequent saves.

**Critical Implementation Requirements:**
1. Store `currentRevisionId` as local signal, updated after each successful save
2. Use `currentRevisionId` (not `selectedNote().revision_id`) as `parent_revision_id` for updates
3. After note creation, immediately set `currentRevisionId` from API response
4. Fall back to `selectedNote()?.revision_id` if `currentRevisionId` is not set

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/lib/store.test.ts` | `should support consecutive saves with updated revision_id` |
| e2e | `e2e/notes.test.ts` | `consecutive PUT should succeed with updated revision_id` |
| e2e | `e2e/notes.test.ts` | `PUT with stale revision_id should return 409 conflict` |

---

### REQ-FE-013: Save Must Actually Persist to Server
**Related Spec**: [06_frontend_backend_interface.md](06_frontend_backend_interface.md) §Data Flow

Save operation MUST send the actual edited content to the server.
Server MUST persist the content to the file system.
Reloading the page MUST show the previously saved content.

**Critical Implementation Requirements:**
1. `handleSave()` MUST pass `editorContent()` to `store.updateNote()`
2. `store.updateNote()` MUST call `noteApi.update()` with correct `markdown` field
3. Backend MUST write content to `content.json` file
4. Backend MUST return success only after content is persisted
5. GET request after save MUST return the updated content

**Failure modes to prevent:**
- Silent early return in `handleSave()` due to missing `revisionId`
- `editorContent()` returning stale/empty value
- API request not including actual content
- Backend not writing to file system
- Backend returning success before write completes

| Test Type | File | Test Name |
|-----------|------|-----------|
| pytest | `backend/tests/test_api.py` | `test_update_note` |
| e2e | `e2e/notes.test.ts` | `saved content should persist after reload (REQ-FE-010)` |

---

## 9. E2E (End-to-End) Requirements

### REQ-E2E-001: Frontend Accessibility
**Related Spec**: [05_security_and_quality.md](05_security_and_quality.md) §2 E2E Tests

Frontend correctly returns HTML.

| Test Type | File | Test Name |
|-----------|------|-----------|
| e2e | `e2e/smoke.test.ts` | `GET / returns HTML with DOCTYPE` |
| e2e | `e2e/smoke.test.ts` | `GET / has correct content-type` |
| e2e | `e2e/smoke.test.ts` | `GET /notes returns HTML` |
| e2e | `e2e/smoke.test.ts` | `GET /about returns HTML` |

---

### REQ-E2E-002: API-Frontend Integration
**Related Spec**: [05_security_and_quality.md](05_security_and_quality.md) §2 E2E Tests

Backend API and frontend work together correctly.

| Test Type | File | Test Name |
|-----------|------|-----------|
| e2e | `e2e/smoke.test.ts` | 全 API Tests |
| e2e | `e2e/notes.test.ts` | 全 Notes CRUD Tests |

---

## 10. Test Execution Commands

### Run All Tests
```bash
mise run test
```

### Individual Package Tests
```bash
# Backend (pytest)
mise run //backend:test

# Frontend (vitest)
mise run //frontend:test

# ieapp-cli (pytest)
mise run //ieapp-cli:test

# E2E tests (bun:test)
mise run e2e
```

### CI/CD Pipeline

| Workflow | Trigger | Purpose |
|----------|---------|---------|
| Python CI | Push, PR to main | Lint (ruff), Type check (ty), Unit tests (pytest) |
| Frontend CI | Push, PR to main | Lint (biome) |
| E2E Tests | Push, PR to main | Full E2E tests with Bun test runner |

---

## 10. Milestone 6 Requirements (Search, Attachments, Canvas Links)

### REQ-M6-001: Search UI Component
**Related Spec**: [02_features_and_stories.md](02_features_and_stories.md) Story 3, [04_api_and_mcp.md](04_api_and_mcp.md) §1 Search

Frontend MUST provide a search input that calls the `/search` endpoint and displays results.

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/components/SearchBar.test.tsx` | `should render search input` |
| vitest | `frontend/src/components/SearchBar.test.tsx` | `should call onSearch when form is submitted` |
| vitest | `frontend/src/components/SearchBar.test.tsx` | `should display search results` |
| e2e | `e2e/notes.test.ts` | `Search functionality E2E test` |

---

### REQ-M6-002: Attachment Upload UI
**Related Spec**: [02_features_and_stories.md](02_features_and_stories.md) Story 6, [04_api_and_mcp.md](04_api_and_mcp.md) §1 Attachments

Editor MUST provide file upload capability and link attachments to notes.

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/components/AttachmentUploader.test.tsx` | `should render file input` |
| vitest | `frontend/src/components/AttachmentUploader.test.tsx` | `should upload file and return attachment` |
| vitest | `frontend/src/components/AttachmentUploader.test.tsx` | `should display uploaded attachments` |
| e2e | `e2e/notes.test.ts` | `Attachment upload E2E test` |

---

### REQ-M6-003: Interactive Canvas with Drag-Drop
**Related Spec**: [02_features_and_stories.md](02_features_and_stories.md) Story 4, [04_api_and_mcp.md](04_api_and_mcp.md) §1 Canvas Links

Canvas view MUST support dragging notes and persisting positions via `canvas_position` in note metadata.

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/components/Canvas.test.tsx` | `should render notes at canvas positions` |
| vitest | `frontend/src/components/Canvas.test.tsx` | `should allow dragging notes` |
| vitest | `frontend/src/components/Canvas.test.tsx` | `should persist position after drag` |
| e2e | `e2e/canvas.test.ts` | `Canvas drag-drop E2E test` |

---

### REQ-M6-004: Link Creation UI
**Related Spec**: [02_features_and_stories.md](02_features_and_stories.md) Story 4, [04_api_and_mcp.md](04_api_and_mcp.md) §1 Canvas Links

Canvas MUST allow users to create bi-directional links between notes.

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/components/Canvas.test.tsx` | `should create link between notes` |
| vitest | `frontend/src/components/Canvas.test.tsx` | `should render links as edges` |
| vitest | `frontend/src/components/Canvas.test.tsx` | `should delete link` |
| e2e | `e2e/canvas.test.ts` | `Link creation E2E test` |

---

### REQ-M6-005: Storage Connector UI
**Related Spec**: [02_features_and_stories.md](02_features_and_stories.md) Story 3, [04_api_and_mcp.md](04_api_and_mcp.md) §1 Workspaces

Workspace settings MUST allow configuring storage backends (local, S3) with validation.

| Test Type | File | Test Name |
|-----------|------|-----------|
| vitest | `frontend/src/components/WorkspaceSettings.test.tsx` | `should display storage config` |
| vitest | `frontend/src/components/WorkspaceSettings.test.tsx` | `should test connection` |
| vitest | `frontend/src/components/WorkspaceSettings.test.tsx` | `should save storage config` |
| e2e | `e2e/workspace.test.ts` | `Storage connector E2E test` |

---

## 11. Requirements Coverage Summary

| Category | Requirements | pytest | vitest | e2e |
|----------|--------------|--------|--------|-----|
| Storage & Data Model | 6 | ✅ | ✅ | - |
| Note Management | 10 | ✅ | ✅ | ✅ |
| Indexer | 7 | ✅ | - | ✅ |
| Integrity | 3 | ✅ | - | - |
| Security | 2 | ✅ | - | - |
| Sandbox | 5 | ✅ | - | - |
| REST API | 3 | ✅ | ✅ | ✅ |
| Frontend | 13 | - | ✅ | ✅ |
| E2E | 2 | - | - | ✅ |
| **Milestone 6** | **5** | - | ✅ | ✅ |
| **Total** | **56** | **35** | **30** | **18** |

---

## Change History

| Date | Version | Changes |
|------|---------|---------|
| 2026-01-02 | 1.3.0 | Added Milestone 6 requirements (REQ-M6-001 through REQ-M6-005) |
| 2025-12-31 | 1.2.0 | Added REQ-FE-013 for save persistence verification; Enhanced REQ-FE-010/011/012 with critical implementation requirements |
| 2025-12-31 | 1.1.0 | Added REQ-FE-010, REQ-FE-011, REQ-FE-012 for editor content persistence |
| 2025-12-31 | 1.0.0 | Initial version created |
