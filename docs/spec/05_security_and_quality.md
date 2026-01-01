# 05. Security, Quality & Testing

## 1. Security Strategy

### Local-Only by Default, Optional Auth When Needed
IEapp ships in a localhost-only mode with no auth prompts to keep the personal workflow frictionless. When the API is exposed beyond the loopback interface (e.g., remote MCP agent, shared lab machine), operators MUST enable authentication (API key, bearer token, or OAuth proxy) before binding to non-local addresses.

### Network Isolation
*   **Localhost Binding**: By default, the API binds ONLY to `127.0.0.1`.
*   **Remote Access**: Blocked by default. Set `IEAPP_ALLOW_REMOTE=true` environment variable to allow remote connections (e.g., in dev containers or Codespaces). This is automatically configured for `mise run dev`.
*   **CORS**: Restricted to the specific frontend origin.

### Data Protection
*   **File Permissions**: The app enforces `chmod 600` on the data directory.
*   **HMAC Signing**: All data revisions are signed with a locally generated key to prevent tampering.
*   **Sanitization**: All inputs (especially in the Code Sandbox) are strictly validated.

### Code Sandbox Security
The `run_script` MCP tool (defined in `04_api_and_mcp.md`) executes arbitrary JavaScript code, requiring strict isolation:
*   **Runtime**: WebAssembly (wasmtime) with a JavaScript engine.
*   **Network Access**: Blocked. All interactions must go through the `host.call` function, which proxies to the internal API.
*   **Filesystem Access**: Blocked. No access to the host filesystem.
*   **Resource Limits**:
    *   **Fuel**: Execution is limited by "fuel" (CPU cycles) to prevent infinite loops.
    *   **Memory**: The Wasm instance has a strict memory limit (e.g., 128MB).
*   **Process Isolation**: Wasmtime runs within the backend process but provides strong memory isolation.

## 2. Testing Strategy (TDD)

We follow a strict Test-Driven Development approach.

### Backend (Python/FastAPI)
*   **Framework**: `pytest`
*   **Unit Tests**: Test `ieapp` library logic, data models, and `fsspec` adapters.
*   **Integration Tests**: Test FastAPI endpoints using `TestClient`.
*   **Contract Tests**: Use `schemathesis` to validate API against OpenAPI spec.
*   **Sandbox Tests**: Verify that `run_script` cannot escape the sandbox or access unauthorized files.

### Frontend (SolidJS)
*   **Framework**: `bun test` (Vitest for unit tests, Bun's native test runner for E2E)
*   **Unit Tests**: Test components and stores using Vitest.
*   **E2E Tests**: TypeScript-based HTTP tests using Bun's native fetch and test runner. Tests verify API endpoints and frontend responses without browser automation.

### Running Tests Locally

#### Unit Tests
Run all unit tests across all packages:
```bash
mise run test
```

Or run tests for individual packages:
```bash
mise run //backend:test    # Backend pytest
mise run //frontend:test   # Frontend vitest
mise run //ieapp-cli:test  # CLI pytest
```

#### E2E Tests
Run full end-to-end tests using Bun's native test runner against a live backend and frontend:
```bash
mise run e2e
```

This command will:
1. Build the sandbox.wasm if needed
2. Start the backend server on port 8000
3. Start the frontend dev server on port 3000
4. Wait for both servers to be ready
5. Execute E2E tests using Bun's test runner
6. Automatically shut down servers when tests complete

For faster iteration during E2E test development, you can run the servers manually in separate terminals:
```bash
# Terminal 1: Backend
mise run //backend:dev

# Terminal 2: Frontend
mise run //frontend:dev

# Terminal 3: Run E2E tests (servers already running)
cd e2e && bun test
```

### CI/CD Pipeline
1.  **Lint**: `ruff` (Python), `biome` (TypeScript/JavaScript).
2.  **Unit**: `pytest` (Backend), `bun test` (Frontend).
3.  **E2E**: `bun test` (TypeScript HTTP tests against live servers in `/e2e` directory).
4.  **Build**: Docker images for deployment, Python wheel for `ieapp` library.

### GitHub Actions Workflows

The repository includes the following CI workflows:

| Workflow | File | Triggers | Purpose |
|----------|------|----------|---------|
| Python CI | `.github/workflows/python-ci.yml` | Push, PR to main | Lint (ruff), type check (ty), unit tests (pytest) |
| Frontend CI | `.github/workflows/frontend-ci.yml` | Push, PR to main | Lint (biome) |
| E2E Tests | `.github/workflows/e2e-ci.yml` | Push, PR to main | Full E2E tests with Bun test runner |

The E2E workflow:
- Starts both backend and frontend servers
- Runs TypeScript HTTP tests using Bun's native test runner
- Tests verify API endpoints and frontend responses
- Has a 30-minute timeout to prevent runaway tests

### Local checks / pre-commit hooks

To keep the repository consistent and to catch issues early, we provide a set of `pre-commit` hooks that run locally before commits:

- **Ruff**: auto-formats and lints Python files. The hook attempts to apply fixes automatically on commit.
- **Ty**: runs the `ty` type checks for the Python projects in the repository (currently `./backend` and `./ieapp-cli`). These run as full-project checks to catch type regressions early.

Install and enable the hooks locally with:

```bash
# Enable the git hook
uvx pre-commit install

# Run all hooks across the repository (recommended before opening a PR)
uvx pre-commit run --all-files
```

Hooks are configured in the repository root file: `.pre-commit-config.yaml`. The CI continues to run `ruff` and `ty` checks on push and PRs; the pre-commit hooks are intended to provide fast local feedback and automatic fixes where applicable.


## 3. Error Handling & Resilience

### Principles
*   **Graceful Degradation**: If S3 is down, the app should still allow viewing cached notes.
*   **Idempotency**: Read/update/delete endpoints (GET/PUT/PATCH/DELETE) MUST remain idempotent, while create endpoints (POST) require either a client-supplied ID or an `Idempotency-Key` header to deduplicate retries. If neither is provided, the server is allowed to reject the request with 409 to prevent accidental duplicates.
*   **Structured Logging**: All errors are logged as JSON with trace IDs.

### Common Error Scenarios
*   **Storage Unavailable**: Return 503, retry with backoff.
*   **Conflict (409)**: Return the server's version so the client can merge.
*   **Sandbox Timeout**: Kill the process and return a specific error to the AI.
