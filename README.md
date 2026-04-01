# Ugoite

**"Local-First Knowledge Space with Resource-First MCP Integration for the Post-SaaS Era"**

## Vision

Ugoite is a knowledge management system built on three core principles:

| Principle    | Description                                                 |
| ------------ | ----------------------------------------------------------- |
| **Low Cost** | No expensive cloud services required; runs on local storage |
| **Easy**     | Markdown-first authoring with Form-defined structure when you need queryable fields |
| **Freedom**  | Your data, your storage, your AI - no vendor lock-in        |

## Start Here

The docsite getting-started flow is the canonical newcomer decision tree. This
README mirrors the same top-level path names so you can choose a first step on
GitHub without comparing two different onboarding maps.

### Choose your first step

- [Understand core concepts](docs/guide/concepts.md) before you choose a
  surface.
- [Try the published release](docs/guide/container-quickstart.md) for the
  fastest browser-based evaluation path.
- [Run from source](docs/guide/docker-compose.md) when you want the current
  backend, frontend, and docsite together; the shortest contributor path is
  `mise run setup`, then `mise run dev`.
- [Use the CLI](docs/guide/cli.md) for terminal-first workflows and scripting.

### After your first step

- **Explore the browser app** by opening `/login` from the published quick
  start or source workflow, then continuing to `/spaces`.
- [Understand auth and access](docs/guide/auth-overview.md) before rollout or
  scripting across the browser, CLI, and API.
- [Read design and source docs](docs/spec/index.md) when you need philosophy,
  requirements, APIs, or machine-readable specs.

Local-first applies most directly to Ugoite's storage model and the CLI's
`core` mode today. The current browser path still needs a running backend +
frontend stack and an explicit login flow, even though the data remains in
user-controlled local storage.

Auth defaults differ by entry path: `mise run dev` uses `passkey-totp` by
default so source contributors exercise the explicit local passkey + 2FA flow,
while the published `docker-compose.release.yaml` quick start uses `mock-oauth`
by default so browser evaluators can reach `/login` and `/spaces` with fewer
setup steps.

Today's shipped AI surface is resource-first MCP access. Read-oriented MCP
resources are available now; broader tool-driven AI workflows remain part of
the `v0.2` roadmap.

## Key Features

- **Markdown as Table**: Markdown stays the authoring surface, while Forms define the canonical fields extracted into Iceberg tables
- **Form Definitions**: Define entry types (Meeting, Task, etc.) with typed fields and templates
- **Resource-First AI Integration**: MCP currently exposes read-oriented resources, with broader AI workflow tooling planned for `v0.2`
- **Local-First Storage**: Your data stays on your device or cloud storage (S3, etc.)
- **Version History**: Every save creates an immutable revision; time travel through your entries

## Stack Overview

| Component    | Technology                           |
| ------------ | ------------------------------------ |
| Frontend     | Bun + SolidStart + TailwindCSS       |
| Backend      | Python 3.12+ (FastAPI)               |
| Core         | Rust (ugoite-core via pyo3 bindings) |
| Storage      | OpenDAL + Apache Iceberg             |
| AI Interface | MCP (resource-first integration today) |

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

## Documentation Map

Start with the user-facing guides:

- [Core Concepts](docs/guide/concepts.md) - Learn what spaces, entries, forms, and search mean before choosing a surface
- [Container Quick Start](docs/guide/container-quickstart.md) - Run published GHCR release images
- [CLI Guide](docs/guide/cli.md) - Install the released CLI or build it from source
- [Local Dev Auth/Login](docs/guide/local-dev-auth-login.md) - Configure local bearer token auth
- [Backend Healthcheck](docs/guide/backend-healthcheck.md) - Quick backend readiness check

Go deeper when you need architecture or implementation contracts:

- [Specification Index](docs/spec/index.md) - Technical specifications
- [Architecture Overview](docs/spec/architecture/overview.md) - System design
- [API Reference](docs/spec/api/rest.md) - REST API documentation

Track ongoing work:

- [Versions Overview](docs/spec/versions/index.md) - Human-readable release streams
  and planned milestones
- [Machine-readable roadmap](docs/version/unknown/roadmap.yaml) - YAML milestone
  and phase status

---

## CLI Quick Start

Install the public `ugoite` npm bootstrap package:

```bash
npm install -g ugoite
ugoite-install
ugoite --help
```

Pin an exact published package version when needed:

```bash
npm install -g ugoite@0.1.0
ugoite-install
ugoite --help
```

The published package metadata lives in `packages/ugoite/package.json`, while
the repository root `package.json` stays private tooling for Husky/commitlint
and release automation.

If you prefer the direct shell bootstrap, install the latest stable `ugoite`
binary with a one-liner:

```bash
curl -fsSL https://raw.githubusercontent.com/ugoite/ugoite/main/scripts/install-ugoite-cli.sh | bash
ugoite --help
```

Pin an exact release when you do not want the newest stable build:

```bash
curl -fsSL https://raw.githubusercontent.com/ugoite/ugoite/main/scripts/install-ugoite-cli.sh | env UGOITE_VERSION=0.1.0 bash
ugoite --help
```

Install an exact release with a platform-specific one-liner:

```bash
# Linux x86_64
curl -fsSL https://github.com/ugoite/ugoite/releases/download/v0.1.0/ugoite-v0.1.0-x86_64-unknown-linux-gnu.install.sh | bash

# Linux arm64
curl -fsSL https://github.com/ugoite/ugoite/releases/download/v0.1.0/ugoite-v0.1.0-aarch64-unknown-linux-gnu.install.sh | bash

# macOS x86_64
curl -fsSL https://github.com/ugoite/ugoite/releases/download/v0.1.0/ugoite-v0.1.0-x86_64-apple-darwin.install.sh | bash

# macOS arm64
curl -fsSL https://github.com/ugoite/ugoite/releases/download/v0.1.0/ugoite-v0.1.0-aarch64-apple-darwin.install.sh | bash
```

For contributor-oriented Cargo workflows, see [CLI Guide](docs/guide/cli.md).

## Setup & Development (mise)

Install dependencies:

```bash
mise run setup
```

Start development (backend + frontend + docsite — `passkey-totp` is the default local auth mode):

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
space, so repeated runs stay predictable. It also prints a terminal progress
bar while entries are generated and verifies `./spaces/<space-id>` exists
before returning success. When `UGOITE_ROOT` is already set for the local dev
stack, the seed task reuses that same root automatically so `mise run seed`
and `mise run dev` keep pointing at the same local storage tree.

Confirm the default dataset after a run:

```bash
cargo run -q -p ugoite-cli -- space list ./spaces
ls "./spaces/${UGOITE_SEED_SPACE_ID:-dev-seed}"
```

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

If only the CLI crate looks stale during local testing, use a package-local
clean rerun instead of wiping the whole shared target tree:

```bash
mise run //ugoite-cli:test:clean
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
bun install --frozen-lockfile
bun dev
```

---

## Dev Container (VS Code) vs Docker Compose (deployment)

Important: this repo provides two distinct container-based workflows:

- Dev Container (development):
  - `.devcontainer/devcontainer.json` creates a reproducible environment for developers, installs `oathtool` for manual TOTP flows, and runs `mise install` as part of the setup.
  - Use Dev Container for onboarding, local development, and consistent dev tooling.

- Docker Compose (deployment / CI):
  - `docker-compose.yaml` is for containerized deployments or CI systems. If you use this for production, verify commands and configuration (e.g., remove `--reload` or `bun dev` and opt for production servers and built frontend assets).

These two environments are separate and intended for different uses—use the Dev Container for development and Docker Compose for deployments.

---

## Container Quick Start (published images)

Start here if you want the quickest way to try a published Ugoite release.
This path uses the shipped release compose file plus published GHCR images and
does not require cloning the repository or building images from source.

Prepare the compose file and `.env`, then pull and start the published stack:

```bash
mkdir -p ugoite-release
cd ugoite-release
curl -fsSLO "https://github.com/ugoite/ugoite/releases/latest/download/docker-compose.release.yaml"
cat > .env <<EOF
UGOITE_VERSION=stable
UGOITE_SPACES_DIR=./spaces
UGOITE_FRONTEND_PORT=3000
UGOITE_BACKEND_PORT=8000
UGOITE_DEV_USER_ID=dev-local-user
UGOITE_DEV_AUTH_PROXY_TOKEN=release-compose-auth-proxy
EOF
mkdir -p ./spaces
docker compose -f docker-compose.release.yaml pull
docker compose -f docker-compose.release.yaml up -d
```

Then open `http://localhost:3000/login`, click **Continue with Mock OAuth**,
and you will land on `/spaces`. The shipped compose file bootstraps the `default` space
at startup so the first browser and CLI session both have a ready workspace.
For more background on the explicit browser login flow, see
[Local Dev Auth Login](docs/guide/local-dev-auth-login.md).

The compose file pulls the canonical release image names used by
`docker-compose.release.yaml`:

- `ghcr.io/ugoite/ugoite/backend:${UGOITE_VERSION}`
- `ghcr.io/ugoite/ugoite/frontend:${UGOITE_VERSION}`

Tag conventions:

- stable releases publish the exact SemVer tag plus `latest` and `stable`
- alpha releases publish the exact prerelease tag plus `alpha`
- beta releases publish the exact prerelease tag plus `beta`

### Environment Variables

| Variable                      | Default                      | Purpose                                                                                                                                                                               |
| ----------------------------- | ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `UGOITE_VERSION`              | `required`                   | Published image tag selector; set it to `stable` or `latest` for the newest stable release, `alpha` or `beta` for the newest prerelease channel, or an exact version to pin the stack |
| `UGOITE_SPACES_DIR`           | `./spaces`                   | Host path mounted into the backend container at `/data`                                                                                                                               |
| `UGOITE_FRONTEND_PORT`        | `3000`                       | Host port that exposes the frontend UI                                                                                                                                                |
| `UGOITE_BACKEND_PORT`         | `8000`                       | Host port that exposes the backend API                                                                                                                                                |
| `UGOITE_DEV_USER_ID`          | `dev-local-user`             | Mock OAuth user id bootstrapped as the shipped quick-start admin-space admin                                                                                                          |
| `UGOITE_DEV_AUTH_PROXY_TOKEN` | `release-compose-auth-proxy` | Shared token wiring between the frontend proxy and backend dev auth flow                                                                                                              |

For more examples, authenticated GHCR pulls, and shutdown steps, see
[Container Quick Start](docs/guide/container-quickstart.md).

If you need the same published topology on Kubernetes, clone the repository and
use the in-repo chart at `charts/ugoite` as described in
[Helm Chart Guide](docs/guide/helm-chart.md). It mirrors the same
frontend/backend image pair, keeps backend storage rooted at `/data`, and
computes the chart-equivalent backend service URL for the frontend.

---

## Docker Compose from source

If you want to build the current workspace from source instead of running the
published release assets above, use this contributor-oriented path.

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

Use the canonical version docs for current roadmap status instead of relying on a
copied milestone list:

- [Versions Overview](docs/spec/versions/index.md) for the current `v0.1` / `v0.2`
  release-stream split
- [v0.1 release stream](docs/spec/versions/v0.1.md) for foundational milestones,
  user-management work, and release preparation
- [v0.2 roadmap](docs/spec/versions/v0.2.md) for user-controlled views and
  AI-enabled / native-app planning

---

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE).

---

## Contributing

Contributions welcome! See [AGENTS.md](AGENTS.md) for development guidelines.

1. Check [docs/tasks/tasks.md](docs/tasks/tasks.md) for current work items
2. Open an issue to discuss larger changes
3. Submit PR with tests
