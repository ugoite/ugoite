# Ugoite

**"Local-First, AI-Native Knowledge Space for the Post-SaaS Era"**

## Vision

Ugoite is a knowledge management system built on three core principles:

| Principle | Description |
|-----------|-------------|
| **Low Cost** | No expensive cloud services required; runs on local storage |
| **Easy** | Markdown-first with automatic structure extraction |
| **Freedom** | Your data, your storage, your AI - no vendor lock-in |

## Key Features

- **Markdown as Table**: Markdown sections map to Form-defined fields stored in Iceberg tables
- **Form Definitions**: Define entry types (Meeting, Task, etc.) with typed fields and templates
- **AI-Programmable**: MCP protocol with resource-first integration for AI agents
- **Local-First Storage**: Your data stays on your device or cloud storage (S3, etc.)
- **Version History**: Every save creates an immutable revision; time travel through your entries

## Stack Overview

| Component | Technology |
|-----------|------------|
| Frontend | Bun + SolidStart + TailwindCSS |
| Backend | Python 3.12+ (FastAPI) |
| Core | Rust (ugoite-core via pyo3 bindings) |
| Storage | OpenDAL + Apache Iceberg |
| AI Interface | MCP (resource-first integration) |

---

## Directory Structure

```
frontend/           # SolidStart frontend
  ├─ src/
  └─ public/
backend/            # FastAPI backend (REST & MCP server)
  └─ src/
ugoite-cli/         # Command-line interface for power users
  └─ src/
ugoite-core/        # Rust core logic + Python bindings
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
- [Backend Healthcheck](docs/guide/backend-healthcheck.md) - Quick backend readiness check
- [Container Quick Start](docs/guide/container-quickstart.md) - Run published GHCR release images
- [CLI Guide](docs/guide/cli.md) - Install the released CLI or build it from source
- [Local Dev Auth/Login](docs/guide/local-dev-auth-login.md) - Configure local bearer token auth
- [Roadmap](docs/tasks/roadmap.md) - Future milestones
- [Current Tasks](docs/tasks/tasks.md) - Active development

---

## CLI Quick Start

Install the latest stable `ugoite` binary with a one-liner:

```bash
curl -fsSL https://raw.githubusercontent.com/ugoite/ugoite/main/scripts/install-ugoite-cli.sh | bash
ugoite --help
```

Pin an exact release when you do not want the newest stable build:

```bash
curl -fsSL https://raw.githubusercontent.com/ugoite/ugoite/main/scripts/install-ugoite-cli.sh | env UGOITE_VERSION=0.1.0 bash
ugoite --help
```

For contributor-oriented Cargo workflows, see [CLI Guide](docs/guide/cli.md).

## Setup & Development (mise)

Install dependencies:

```bash
mise run setup
```

Start development (backend + frontend + docsite — `manual-totp` is the default local auth mode):

```bash
mise run dev
```

Seed a local demo space with sample data:

```bash
mise run seed
```

Override the default space, scenario, entry count, or RNG seed when you need a
different local dataset:

```bash
UGOITE_SEED_SPACE_ID=ux-demo UGOITE_SEED_SCENARIO=supply-chain \
UGOITE_SEED_ENTRY_COUNT=25 UGOITE_SEED_VALUE=42 mise run seed
mise run seed:scenarios
```

The seed task wraps the existing Rust CLI sample-data command, keeps builds in
the shared `target/rust` cache, and refuses to overwrite an existing local
space, so repeated runs stay predictable.

If Rust build artifacts grow unexpectedly during local development, clear the
shared Rust target cache and the legacy ugoite-core cache path:

```bash
mise run cleanup:rust-targets
```

If only the editable `ugoite-core` extension looks stale, use a package-local
clean rebuild without wiping the entire shared target tree:

```bash
mise run //ugoite-core:build:clean
```

See [Local Dev Auth/Login](docs/guide/local-dev-auth-login.md) for the
canonical `mise run dev` workflow, including the explicit `/login` browser
flow, refreshing the local login context, supported auth modes, and the
`dev:backend`, `dev:frontend`, or `dev:docsite` shortcuts when needed. See
[CLI Guide](docs/guide/cli.md) for the direct sample-data commands behind
`mise run seed`.

Important: During development we expect `BACKEND_URL` to be set to the backend host reachable from the dev server (e.g. `http://localhost:8000`). The frontend dev server proxies `/api` requests to this URL. Client code uses `/api` to access the backend.
When running with `docker-compose`, we set: `BACKEND_URL=http://backend:8000`.

Details:

Backend (dev) example:

```bash
cd backend
uv run uvicorn src.app.main:app --reload --host 0.0.0.0 --port 8000
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

## Container Quick Start (published images)

Pull and run a published release from GHCR:

```bash
mkdir -p spaces
UGOITE_VERSION=0.0.1 docker compose -f docker-compose.release.yaml pull
UGOITE_VERSION=0.0.1 docker compose -f docker-compose.release.yaml up -d
```

Published images:

- `ghcr.io/ugoite/ugoite/backend`
- `ghcr.io/ugoite/ugoite/frontend`

Tag conventions:

- stable releases publish the exact SemVer tag plus `latest` and `stable`
- alpha releases publish the exact prerelease tag plus `alpha`
- beta releases publish the exact prerelease tag plus `beta`

For more examples, direct `docker pull` commands, and shutdown steps, see
[Container Quick Start](docs/guide/container-quickstart.md).

---

## Docker Compose from source

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

Run all tests from repo root:

```bash
mise run test
```

Run the authoritative local E2E suite. It prefers the docker-compose path used
by GitHub Actions when Docker is available, and otherwise falls back to a
production-style host runner with the same Playwright JUnit/no-skips gates:

```bash
mise run e2e
```

For faster local iteration against direct dev servers (not CI parity):

```bash
mise run e2e:dev
```

Where you can run this:

- Dev Container: everything needed to run tests is available; run `mise run test`.
- GitHub Actions `python-ci`: runs `ruff`, `ty`, and `pytest` for `backend/` and `ugoite-cli/`.
- Local (non-container): install `uv`, then run the commands above.

Frontend tests: check `frontend/package.json`.

---

## Known Issues & Future Work

See [Roadmap](docs/tasks/roadmap.md) for planned milestones:

- **Milestone 2** (Completed): Codebase unification, Rust core library
- **Milestone 3**: Full AI integration, vector search
- **Milestone 4** (Phase 1/2 completed): User management, authentication hardening and follow-up tasks
- **Milestone 5**: Native desktop app (Tauri)

---

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE).

---

## Contributing

Contributions welcome! See [AGENTS.md](AGENTS.md) for development guidelines.

1. Check [docs/tasks/tasks.md](docs/tasks/tasks.md) for current work items
2. Open an issue to discuss larger changes
3. Submit PR with tests
