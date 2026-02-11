# Ugoite Backend

FastAPI-based REST API for Ugoite - your AI-native, programmable knowledge base.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                    FastAPI Application                   │
├─────────────┬─────────────────┬────────────────────────┤
│   REST API  │   MCP Server    │     Middleware         │
│  /api/*     │  (Milestone 4)  │  - HMAC Signing        │
│             │                 │  - Localhost Guard      │
│             │                 │  - Error Handling       │
├─────────────┴─────────────────┴────────────────────────┤
│                   ugoite-core Library                     │
│  - space.py      (Space CRUD)                            │
│  - entries.py    (Entry CRUD + Revision Control)         │
│  - indexer.py    (Structure-from-Text Extraction)        │
├─────────────────────────────────────────────────────────┤
│                   File System Storage                    │
│  global.json → spaces/{id}/meta.json + forms/            │
└─────────────────────────────────────────────────────────┘
```

## Module Structure

```
src/app/
├── main.py              # FastAPI app entry point
├── api/
│   ├── api.py           # Router aggregation
│   └── endpoints/
│       └── spaces.py     # REST endpoints for spaces & entries
├── core/
│   ├── config.py        # Configuration (root path, settings)
│   ├── middleware.py    # Security middleware (HMAC, localhost)
│   └── security.py      # Auth utilities
├── mcp/
│   └── server.py        # MCP protocol server (Milestone 4)
└── models/
    └── payloads.py      # Pydantic request/response models
```

## Key Design Decisions

### 1. Dependency on ugoite-core Library

The backend does NOT implement business logic directly. Instead, it delegates to `ugoite-core`:

```python
# ✅ Correct: Use library functions
from ugoite.entries import create_entry, update_entry, get_entry
from ugoite.space import create_space, list_spaces

# ❌ Wrong: Direct file manipulation in API layer
```

### 2. Optimistic Concurrency Control

All entry updates require `parent_revision_id` for conflict detection:

```python
# Update endpoint returns 409 if revision mismatch
@router.put("/spaces/{space_id}/entries/{entry_id}")
async def update_entry_endpoint(payload: EntryUpdate):
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
| Create entry | `{"id": "...", "revision_id": "..."}` |
| Update entry | `{"id": "...", "revision_id": "..."}` |
| Get entry | Full entry object with content |
| List entries | Array of EntryRecord (index data) |

### 4. Security Middleware

- **Localhost Guard**: Rejects requests from non-localhost unless explicitly configured
- **HMAC Signing**: All responses include `X-Ugoite-Signature` header for integrity verification

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
| GET | `/spaces` | List all spaces |
| POST | `/spaces` | Create space |
| GET | `/spaces/{id}` | Get space metadata |
| GET | `/spaces/{id}/entries` | List entries (index data) |
| POST | `/spaces/{id}/entries` | Create entry |
| GET | `/spaces/{id}/entries/{entryId}` | Get full entry |
| PUT | `/spaces/{id}/entries/{entryId}` | Update entry |
| DELETE | `/spaces/{id}/entries/{entryId}` | Delete (tombstone) entry |
| POST | `/spaces/{id}/query` | Query entries by filter |

## Testing Strategy

Following TDD approach from [docs/spec/testing/strategy.md](../docs/spec/testing/strategy.md) and [docs/tasks/tasks.md](../docs/tasks/tasks.md):

1. **Unit Tests**: Test library functions in isolation
2. **API Tests**: TestClient-based endpoint testing
3. **Integration Tests**: Full request cycle with temp filesystem

Test fixtures are in `tests/conftest.py`.

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `UGOITE_ROOT` | `~/.ugoite` | Root path for space storage |
| `UGOITE_ALLOW_REMOTE` | `false` | Allow non-localhost connections |
