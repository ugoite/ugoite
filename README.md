# IE-app

## Overview

This repository provides a sample knowledge-management web app (IE-app) as a demo project.

Stack overview:
- Frontend: Bun + SolidStart
- Backend: Python (FastAPI) + fsspec

---

## Directory structure

```
frontend/
  ├─ src/
  ├─ public/
backend/
  ├─ src/
  ├─ requirements.txt
ieapp-cli/
docs/
tests/
README.md
```

---

## Setup & Development (mise)

Install dependencies:

```bash
mise run install
```

Start development (frontend + backend):

```bash
mise run dev
```

Note: During development we expect `BACKEND_URL` to be set to the backend host reachable from the dev server (e.g. `http://localhost:8000`). The frontend dev server proxies `/api` requests to this URL. Client code uses `/api` to access the backend.
When running with `docker-compose`, we set: `BACKEND_URL=http://backend:8000`.

Notes:

Backend (dev) example:

```bash
cd backend
uvicorn backend.main:app --reload --host 0.0.0.0 --port 8000
```

Frontend (dev) example:

```bash
cd frontend
bun install
bun dev
```

---

## Dev Container (VS Code) vs Docker Compose (deployment)

Important: this repo provides two distinct container-based workflows:

- Dev Container (development):
  - `.devcontainer/devcontainer.json` creates a reproducible environment for developers and runs `mise install` as part of the setup.
  - Use Dev Container for onboarding, local development, and consistent dev tooling.

- Docker Compose (deployment / CI):
  - `docker-compose.yaml` is for containerized deployments or CI systems. If you use this for production, verify commands and configuration (e.g., remove `--reload` or `bun dev` and opt for production servers and built frontend assets).

These two environments are separate and intended for different uses—use the Dev Container for development and Docker Compose for deployments.

---

## Docker Compose

Start services with:

```bash
docker compose up --build
```

Run detached:

```bash
docker compose up -d --build
```

---

## Tests

Run backend tests:

```bash
cd backend
uv run pytest
```

Run all Python tests from repo root:

```bash
uv run pytest
```

### Sandbox (Wasm) prerequisites

The backend includes a WebAssembly-based JavaScript sandbox (`backend/src/app/sandbox/python_sandbox.py`).

- For unit tests: `pytest` works without building the Wasm artifact (it uses a minimal fallback runner when `sandbox.wasm` is missing).
- For full sandbox execution (real Wasmtime + Javy): build `backend/src/app/sandbox/sandbox.wasm`.

Build the Wasm artifact:

```bash
# Option A: via mise (recommended)
mise run sandbox:build

# Option B: direct script
bash scripts/setup_javy.sh
bash backend/src/app/sandbox/build_sandbox_wasm.sh
```

Where you can run this:

- Dev Container: everything needed to run tests is available; run `uv run pytest`.
- GitHub Actions `python-ci`: runs `ruff`, `ty`, and `pytest` for `backend/` and `ieapp-cli/`.
- Local (non-container): install `uv`, then run the commands above. If you want the real Wasm sandbox, build `sandbox.wasm` and place it at `backend/src/app/sandbox/sandbox.wasm`.

Frontend tests: check `frontend/package.json`.

---

## Known issues & TODO

- Authentication (JWT/OAuth) is not implemented yet.
- Production-grade persistence (e.g. Postgres) is not configured.

---

## Contributing

Contributions welcome. Open an issue to discuss larger changes before submitting a PR.

---
