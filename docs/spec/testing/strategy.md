# Testing Strategy

## Philosophy

Ugoite follows **Test-Driven Development (TDD)**:

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
| ugoite-minimum | cargo test | `ugoite-minimum/tests/` |
| ugoite-core | cargo test + pytest | `ugoite-core/tests/` |
| ugoite-cli | cargo test | `ugoite-cli/tests/` |
| backend | pytest | `backend/tests/` |
| frontend | vitest | `frontend/src/**/*.test.ts(x)` |
| docsite | vitest | `docsite/src/**/*.test.ts` |

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
mise run //ugoite-minimum:test # Portable Rust core tests with 100% coverage enforcement
mise run //ugoite-core:test    # OpenDAL adapter + Python binding tests
mise run //backend:test    # Backend pytest
mise run //frontend:test   # Frontend vitest
mise run //docsite:test    # Docsite Vitest unit tests
mise run //ugoite-cli:test # Incremental CLI Rust tests
mise run //ugoite-cli:test:coverage # CLI Rust coverage gate matching CI
mise run //ugoite-cli:test:clean # Clean package-local CLI artifacts and rerun tests
```

Root `mise run test`, `mise run //ugoite-cli:test:coverage`, pre-commit, and
Rust CI enforce the CLI 100% line-coverage gate via `cargo llvm-cov`.

### E2E Tests
```bash
mise run e2e
```

This command:
1. Prefers the Docker Compose path used in CI when Docker is available locally
2. Falls back to a production-style host runner when Docker is unavailable
3. Uses CI-equivalent Playwright JUnit/no-skipped-tests gates in either mode
4. Executes the full Playwright E2E suite
5. Shuts down any services it started

### Fast E2E Iteration
```bash
# Single-command fast path (not CI parity)
mise run e2e:dev

# Terminal 1: Backend
mise run //backend:dev

# Terminal 2: Frontend
mise run //frontend:dev

# Terminal 3: Run E2E tests
cd e2e && npm run test
```

Use `mise run e2e` before pushing when you need CI-equivalent validation. Use
`mise run e2e:dev`, `mise run e2e:smoke`, or the three-terminal flow above when
you need a faster local feedback loop.

## Coverage Requirements

| Module | Target | Current |
|--------|--------|---------|
| ugoite-minimum | 100% | 100% (enforced) |
| ugoite-cli | 100% | 100% (enforced) |
| backend | >80% | ~75% |
| frontend | >70% | ~70% |
| docsite | 100% | 100% (enforced) |
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
ugoite-minimum/tests/
├── test_coverage.rs     # Portable coverage-focused tests for storage/integrity/metadata
└── ...

ugoite-core/tests/
├── test_space.rs        # Space adapter tests
├── test_entry.rs        # Entry tests
└── ...

ugoite-cli/tests/
├── test_space.rs        # Space tests
├── test_entries.rs      # Entry tests
├── test_indexer.rs      # Indexer tests
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

docsite/src/lib/
├── spec-data.test.ts    # Spec/YAML loading tests
├── doc-links.test.ts    # Internal link resolution tests
└── navigation.test.ts   # Navigation tree tests
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

Rust tests in `ugoite-minimum/tests/` and `ugoite-core/tests/` use a
requirement-aware naming convention:

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
- Shared storage abstraction via the `memory://` backend (through
  `ugoite-core`'s OpenDAL-backed adapter layer)
- No external service mocks needed

### Frontend
- Mock API responses with vitest mocks
- Component isolation with test utilities

### E2E
- Real servers, real HTTP
- Test database reset between tests
