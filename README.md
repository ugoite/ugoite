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

Notes:
- `mise run dev` is configured in `mise.toml` and runs `//backend:dev` and `//frontend:dev`.
- To run services individually, see the examples below.

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
pytest
```

Frontend tests: check `frontend/package.json`.

---

## Known issues & TODO

- Authentication (JWT/OAuth) is not implemented yet.
- Production-grade persistence (e.g. Postgres) is not configured.

---

## Contributing

Contributions welcome. Open an issue to discuss larger changes before submitting a PR.

---
