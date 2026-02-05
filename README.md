# IEapp

**"Local-First, AI-Native Knowledge Space for the Post-SaaS Era"**

## Vision

IEapp is a knowledge management system built on three core principles:

| Principle | Description |
|-----------|-------------|
| **Low Cost** | No expensive cloud services required; runs on local storage |
| **Easy** | Markdown-first with automatic structure extraction |
| **Freedom** | Your data, your storage, your AI - no vendor lock-in |

## Key Features

- **Markdown as Database**: Write standard Markdown with `## Headers` that become structured fields
- **Form Definitions**: Define entry types (Meeting, Task, etc.) with typed fields and templates
- **AI-Programmable**: MCP protocol with resource-first integration for AI agents
- **Local-First Storage**: Your data stays on your device or cloud storage (S3, etc.)
- **Version History**: Every save creates an immutable revision; time travel through your entries

## Stack Overview

| Component | Technology |
|-----------|------------|
| Frontend | Bun + SolidStart + TailwindCSS |
| Backend | Python 3.12+ (FastAPI) |
| Storage | fsspec (local, S3, memory) |
| AI Interface | MCP (resource-first integration) |

---

## Directory Structure

```
frontend/           # SolidStart frontend
  ├─ src/
  └─ public/
backend/            # FastAPI backend (REST & MCP server)
  └─ src/
ieapp-cli/          # Command-line interface for power users
  └─ src/
docs/
  ├─ spec/          # Technical specifications (YAML + Markdown)
  └─ tasks/         # Milestone tracking and roadmap
e2e/                # End-to-end tests (Bun)
```

---

## Documentation

- [Specification Index](docs/spec/index.md) - Technical specifications
- [Architecture Overview](docs/spec/architecture/overview.md) - System design
- [API Reference](docs/spec/api/rest.md) - REST API documentation
- [Roadmap](docs/tasks/roadmap.md) - Future milestones
- [Current Tasks](docs/tasks/tasks.md) - Active development

---

## Setup & Development (mise)

Install dependencies:

```bash
mise run setup
```

Start development (frontend + backend):

```bash
mise run dev
```

Important: During development we expect `BACKEND_URL` to be set to the backend host reachable from the dev server (e.g. `http://localhost:8000`). The frontend dev server proxies `/api` requests to this URL. Client code uses `/api` to access the backend.
When running with `docker-compose`, we set: `BACKEND_URL=http://backend:8000`.

Details:

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

Where you can run this:

- Dev Container: everything needed to run tests is available; run `uv run pytest`.
- GitHub Actions `python-ci`: runs `ruff`, `ty`, and `pytest` for `backend/` and `ieapp-cli/`.
- Local (non-container): install `uv`, then run the commands above.

Frontend tests: check `frontend/package.json`.

---

## Known Issues & Future Work

See [Roadmap](docs/tasks/roadmap.md) for planned milestones:

- **Milestone 2** (In Progress): Codebase unification, Rust core library
- **Milestone 3**: Full AI integration, vector search
- **Milestone 4**: User management, authentication
- **Milestone 5**: Native desktop app (Tauri)

---

## Contributing

Contributions welcome! See [AGENTS.md](AGENTS.md) for development guidelines.

1. Check [docs/tasks/tasks.md](docs/tasks/tasks.md) for current work items
2. Open an issue to discuss larger changes
3. Submit PR with tests
