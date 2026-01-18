# Testing Strategy

## Philosophy

IEapp follows **Test-Driven Development (TDD)**:

1. Write failing test first
2. Implement minimal code to pass
3. Refactor while keeping tests green

## Test Pyramid

```
        ╱╲
       ╱E2E╲         Few: Critical user flows
      ╱──────╲
     ╱Integr.╲       Some: API + component tests
    ╱──────────╲
   ╱   Unit     ╲    Many: Fast, isolated tests
  ╱──────────────╲
```

## Test Types

### Unit Tests

| Module | Framework | Location |
|--------|-----------|----------|
| ieapp-cli | pytest | `ieapp-cli/tests/` |
| backend | pytest | `backend/tests/` |
| frontend | vitest | `frontend/src/**/*.test.ts(x)` |

### Integration Tests

- Backend: FastAPI TestClient with memory filesystem
- Frontend: Component tests with mocked API

### End-to-End Tests

| Framework | Location | Description |
|-----------|----------|-------------|
| bun:test | `e2e/` | TypeScript HTTP tests against live servers |

## Running Tests

### All Tests
```bash
mise run test
```

### Individual Packages
```bash
mise run //backend:test    # Backend pytest
mise run //frontend:test   # Frontend vitest
mise run //ieapp-cli:test  # CLI pytest
```

### E2E Tests
```bash
mise run e2e
```

This command:
1. Starts backend server on port 8000
2. Starts frontend server on port 3000
3. Waits for servers to be ready
4. Executes E2E tests
5. Shuts down servers

### Fast E2E Iteration
```bash
# Terminal 1: Backend
mise run //backend:dev

# Terminal 2: Frontend
mise run //frontend:dev

# Terminal 3: Run E2E tests
cd e2e && bun test
```

## Coverage Requirements

| Module | Target | Current |
|--------|--------|---------|
| ieapp-cli | >80% | ~85% |
| backend | >80% | ~75% |
| frontend | >70% | ~70% |
| e2e | Critical paths | Complete |

## Test Organization

### Naming Convention

```python
# Pattern: test_<feature>_<scenario>
def test_workspace_create_success():
    ...

def test_workspace_create_duplicate_returns_409():
    ...
```

### Requirement-aware Naming Convention

For pytest tests that validate a specific requirement, use:

```python
# Pattern: test_<feature>_<requirement_id>_<description>
# Example (REQ-API-001):
def test_api_req_api_001_create_workspace():
    """REQ-API-001: Workspace CRUD"""
    ...
```

Notes:

- Replace hyphens in `REQ-XXX-001` with underscores to form `req_xxx_001`.
- `feature` should reflect the domain under test (e.g., api, note, sto, cls).

### Test Files

```
ieapp-cli/tests/
├── conftest.py          # Shared fixtures
├── test_workspace.py    # Workspace tests
├── test_notes.py        # Note tests
├── test_indexer.py      # Indexer tests
└── ...

backend/tests/
├── conftest.py          # Shared fixtures
├── test_api.py          # API endpoint tests
├── test_api_memory.py   # Memory filesystem tests
└── ...

frontend/src/
├── lib/
│   ├── store.test.ts    # Store tests
│   └── client.test.ts   # API client tests
└── components/
    └── *.test.tsx       # Component tests
```

## Requirements Traceability

Every test should map to a requirement:

```python
def test_note_create_basic():
    """REQ-NOTE-001: Note Creation"""
    ...
```

Verification tests in `docs/tests/` ensure coverage.

## Mocking Strategy

### Backend
- Memory filesystem via fsspec `memory://` protocol
- No external service mocks needed

### Frontend
- Mock API responses with vitest mocks
- Component isolation with test utilities

### E2E
- Real servers, real HTTP
- Test database reset between tests
