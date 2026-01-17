# IEapp Backend

FastAPI-based REST API for IEapp - your AI-native, programmable knowledge base.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                    FastAPI Application                   │
├─────────────┬─────────────────┬────────────────────────┤
│   REST API  │   MCP Server    │     Middleware         │
│ /workspaces/* │  (Milestone 4)  │  - HMAC Signing        │
│             │                 │  - Localhost Guard      │
│             │                 │  - Error Handling       │
├─────────────┴─────────────────┴────────────────────────┤
│                   ieapp-core Library                     │
│  - workspace.py  (Workspace CRUD)                        │
│  - notes.py      (Note CRUD + Revision Control)          │
│  - indexer.py    (Structure-from-Text Extraction)        │
├─────────────────────────────────────────────────────────┤
│                   File System Storage                    │
│  global.json → workspaces/{id}/meta.json + notes/        │
└─────────────────────────────────────────────────────────┘
```

## Module Structure

```
src/app/
├── main.py              # FastAPI app entry point
├── api/
│   ├── api.py           # Router aggregation
│   └── endpoints/
│       └── workspaces.py # REST endpoints for workspaces & notes
├── core/
│   ├── config.py        # Configuration (root path, settings)
│   ├── middleware.py    # Security middleware (HMAC, localhost)
│   └── security.py      # Auth utilities
├── mcp/
│   └── server.py        # MCP protocol server (Milestone 4)
└── models/
    └── classes.py       # Pydantic request/response models
```

## Key Design Decisions

### 1. Dependency on ieapp-core Library

The backend does NOT implement business logic directly. Instead, it delegates to `ieapp-core`:

```python
# ✅ Correct: Use library functions
from ieapp.notes import create_note, update_note, get_note
from ieapp.workspace import create_workspace, list_workspaces

# ❌ Wrong: Direct file manipulation in API layer
```

### 2. Optimistic Concurrency Control

All note updates require `parent_revision_id` for conflict detection:

```python
# Update endpoint returns 409 if revision mismatch
@router.put("/workspaces/{workspace_id}/notes/{note_id}")
async def update_note_endpoint(payload: NoteUpdate):
    try:
        update_note(ws_path, note_id, payload.markdown, payload.parent_revision_id)
    except RevisionMismatchError as e:
        raise HTTPException(status_code=409, detail={
            "error": "revision_conflict",
            "current_revision_id": e.current_revision
        })
```

### 3. Response Formats

| Operation | Response |
|-----------|----------|
| Create note | `{"id": "...", "revision_id": "..."}` |
| Update note | `{"id": "...", "revision_id": "..."}` |
| Get note | Full note object with content |
| List notes | Array of NoteRecord (index data) |

### 4. Security Middleware

- **Localhost Guard**: Rejects requests from non-localhost unless explicitly configured
- **HMAC Signing**: All responses include `X-IEApp-Signature` header for integrity verification

## Getting Started

### Prerequisites

- Python 3.12+
- uv (package manager)

### Installation

```bash
cd backend
uv sync
```

### Development

```bash
# Start development server
uv run uvicorn app.main:app --reload --host 127.0.0.1 --port 8000
```

### Testing

```bash
# Run all tests
uv run pytest

# Run with coverage
uv run pytest --cov=app --cov-report=html
```

### Linting

```bash
uv run ruff check .
uv run ruff format .
```

## API Endpoints

See [docs/spec/api/rest.md](../docs/spec/api/rest.md) and [docs/spec/api/mcp.md](../docs/spec/api/mcp.md) for the API specification.

### Quick Reference

| Method | Path | Description |
|--------|------|-------------|
| GET | `/workspaces` | List all workspaces |
| POST | `/workspaces` | Create workspace |
| GET | `/workspaces/{id}` | Get workspace metadata |
| GET | `/workspaces/{id}/notes` | List notes (index data) |
| POST | `/workspaces/{id}/notes` | Create note |
| GET | `/workspaces/{id}/notes/{noteId}` | Get full note |
| PUT | `/workspaces/{id}/notes/{noteId}` | Update note |
| DELETE | `/workspaces/{id}/notes/{noteId}` | Delete (tombstone) note |
| POST | `/workspaces/{id}/query` | Query notes by filter |

## Testing Strategy

Following TDD approach from [docs/spec/testing/strategy.md](../docs/spec/testing/strategy.md) and [docs/tasks/tasks.md](../docs/tasks/tasks.md):

1. **Unit Tests**: Test library functions in isolation
2. **API Tests**: TestClient-based endpoint testing
3. **Integration Tests**: Full request cycle with temp filesystem

Test fixtures are in `tests/conftest.py`.

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `IEAPP_ROOT` | `~/.ieapp` | Root path for workspace storage |
| `IEAPP_ALLOW_REMOTE` | `false` | Allow non-localhost connections |
