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

Note: During development we expect `DEV_BACKEND_URL` to be set to a path such as `/api` and `DEV_BACKEND_PROXY_TARGET` to point to the backend host reachable from the dev server. The `VITE_BACKEND_URL` (exposed to client code) is set to `/api` in development to make frontend calls same-origin; in production builds set it to a public API URL if needed.
When running with `docker-compose`, we set: `DEV_BACKEND_URL=/api` and `DEV_BACKEND_PROXY_TARGET=http://backend:8000`. Do not set `VITE_BACKEND_URL` to container-only hostnames (such as `http://backend:8000`) when creating client builds because those won't resolve for users outside the containerized network. For Codespaces use the `/api` proxy approach.
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
