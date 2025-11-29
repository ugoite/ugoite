# 05. Security, Quality & Testing

## 1. Security Strategy

### "No Auth" Context
IEapp is designed for local/personal use. However, security is still critical.

### Network Isolation
*   **Localhost Binding**: By default, the API binds ONLY to `127.0.0.1`.
*   **CORS**: Restricted to the specific frontend origin.

### Data Protection
*   **File Permissions**: The app enforces `chmod 600` on the data directory.
*   **HMAC Signing**: All data revisions are signed with a locally generated key to prevent tampering.
*   **Sanitization**: All inputs (especially in the Code Sandbox) are strictly validated.

## 2. Testing Strategy (TDD)

We follow a strict Test-Driven Development approach.

### Backend (Python/FastAPI)
*   **Framework**: `pytest`
*   **Unit Tests**: Test `ieapp` library logic, data models, and `fsspec` adapters.
*   **Integration Tests**: Test FastAPI endpoints using `TestClient`.
*   **Contract Tests**: Use `schemathesis` to validate API against OpenAPI spec.
*   **Sandbox Tests**: Verify that `run_python_script` cannot escape the sandbox or access unauthorized files.

### Frontend (SolidJS)
*   **Framework**: `bun test` + `Playwright`
*   **Unit Tests**: Test components and stores.
*   **E2E Tests**: Use Playwright to drive the full application, verifying the "Optimistic UI" behavior and Canvas interactions.

### CI/CD Pipeline
1.  **Lint**: `ruff` (Python), `biome` (TypeScript/JavaScript).
2.  **Unit**: `pytest` (Backend), `bun test` (Frontend).
3.  **E2E**: `playwright` (headless browser tests).
4.  **Build**: Docker images for deployment, Python wheel for `ieapp` library.

## 3. Error Handling & Resilience

### Principles
*   **Graceful Degradation**: If S3 is down, the app should still allow viewing cached notes.
*   **Idempotency**: All API operations (except `run_python_script`) should be idempotent.
*   **Structured Logging**: All errors are logged as JSON with trace IDs.

### Common Error Scenarios
*   **Storage Unavailable**: Return 503, retry with backoff.
*   **Conflict (409)**: Return the server's version so the client can merge.
*   **Sandbox Timeout**: Kill the process and return a specific error to the AI.
