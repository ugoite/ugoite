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
| Playwright | `e2e/` | TypeScript tests against live servers |

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
cd e2e && npm run test
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
def test_space_create_success():
    ...

def test_space_create_duplicate_returns_409():
    ...
```

### Requirement-aware Naming Convention

For pytest tests that validate a specific requirement, use:

```python
# Pattern: test_<feature>_<requirement_id>_<description>
# Example (REQ-API-001):
def test_api_req_api_001_create_space():
    """REQ-API-001: Space CRUD"""
    ...
```

Details:

- Replace hyphens in `REQ-XXX-001` with underscores to form `req_xxx_001`.
- `feature` should reflect the domain under test (e.g., api, entry, sto, form).

### Test Files

```
ieapp-cli/tests/
├── conftest.py          # Shared fixtures
├── test_space.py        # Space tests
├── test_entries.py      # Entry tests
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
def test_entry_create_basic():
    """REQ-ENTRY-001: Entry Creation"""
    ...
```

Verification tests in `docs/tests/` ensure coverage.

### Requirement-aware Naming Convention

Requirement traceability is enforced through the requirements YAML and REQ
references in tests. Python tests generally keep descriptive names and embed the
REQ ID in docstrings or comments:

```python
def test_all_requirements_have_tests() -> None:
    """REQ-API-005: Requirements must list tests and files must exist."""
    ...
```

Rust tests in `ieapp-core/tests/` use a requirement-aware naming convention:

```rust
async fn test_entry_req_entry_001_create_entry_basic() -> anyhow::Result<()> {
    // REQ-ENTRY-001
    ...
}
```

When using this pattern, replace hyphens in `REQ-XXX-001` with underscores to
form `req_xxx_001`. The `feature` segment should reflect the domain under test
(e.g., api, space, entry, form, core, cli).

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
