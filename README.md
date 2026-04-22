# Ugoite

**"Local-First Knowledge Space with Resource-First MCP Integration for the Post-SaaS Era"**

> **Positioning today:** local-first most directly describes Ugoite's storage
> model and CLI `core` path today. The current browser route is still
> server-backed, runs through the backend + frontend stack, and requires an
> explicit `/login` flow.

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

> **Browser path today:** the current browser route still needs a running
> backend + frontend stack and an explicit `/login` flow. If you want the
> lowest-setup-cost local-first path, start with the CLI in `core` mode.

### Choose your first step

- [Try the published release](docs/guide/container-quickstart.md) for the
  fastest browser-based evaluation path, while still running the browser stack
  with an explicit login step.
- [Run from source](docs/guide/local-dev-auth-login.md) when you want the current
  backend, frontend, and docsite together; choose the repo Devcontainer /
  GitHub Codespaces path when you want the preloaded contributor environment
  (`mise`, `gh`, `oathtool`, `mise install`, `mise run setup`, and
  `npx playwright install --with-deps chromium`), or run `mise run setup` on
  your host when you already manage the toolchain yourself; both paths continue
  with `mise run dev`, followed by the explicit `/login` flow.
  If you intentionally use the repo-root `docker compose up --build` path
  instead, export `UGOITE_DEV_SIGNING_SECRET` and
  `UGOITE_DEV_AUTH_PROXY_TOKEN` first with at least 32 characters of random
  secret material or startup will fail fast. The exact commands live in the
  [Docker Compose Guide](docs/guide/docker-compose.md).
- [Use the CLI](docs/guide/cli.md) for terminal-first workflows and scripting.

If you only need the portable Rust layer for WASM, embedding, or pure helper
work, start with [`ugoite-minimum`](ugoite-minimum/README.md) and the portable
contributor notes in [Contributor Workflow](CONTRIBUTING.md).

### After your first step

- [Understand core concepts](docs/guide/concepts.md) when you want the mental
  model behind spaces, entries, forms, and search before you go deeper into
  auth or the specs.
- [Create your first space, form, and entry](docs/guide/browser-first-entry.md)
  once `/login` succeeds and you want the exact `/spaces` -> form -> entry path.
- [Understand auth and access](docs/guide/auth-overview.md) before rollout or
  scripting across the browser, CLI, and API.
- [Read design and source docs](docs/spec/index.md) when you need philosophy,
  requirements, APIs, or machine-readable specs.

For a brand-new browser space, use the
[Browser Walkthrough](docs/guide/browser-first-entry.md) when you want the
concrete first productive in-app sequence after login.

Local-first applies most directly to Ugoite's storage model and the CLI's
`core` mode today. The current browser path still needs a running backend +
frontend stack and an explicit login flow, even though the data remains in
user-controlled local storage.

Auth defaults differ by entry path: `mise run dev` uses `passkey-totp` by
default so source contributors exercise the explicit local passkey + 2FA flow,
while the published `docker-compose.release.yaml` quick start uses the local
demo login mode (`mock-oauth`) by default so browser evaluators can reach
`/login` and `/spaces` with fewer steps and no external provider. See the
[canonical auth reference](docs/guide/local-dev-auth-login.md) for the
`passkey-totp` vs `mock-oauth` comparison, the explicit `/login` mental model,
and why source and published defaults differ.

### Which entry path should you choose?

| Path | Best when | Setup cost / requirements | Trade-off |
| --- | --- | --- | --- |
| [Try the published release](docs/guide/container-quickstart.md) | You want the fastest visual evaluation of the published browser experience | Medium: Docker + published image pulls + frontend/backend containers + explicit login | Browser-first, but still multi-service and login-gated |
| [Use the CLI](docs/guide/cli.md) in `core` mode | You want the lightest local-first workflow with direct filesystem access | Lowest: released CLI install + local filesystem path; no container stack required | Terminal-first experience; no browser UI or server-backed collaboration features |
| [Work on `ugoite-minimum`](ugoite-minimum/README.md) | You are contributing portable Rust, WASM-oriented, or embedding-friendly logic without the full app stack | Medium: source checkout + `mise run setup`, then package-local `//ugoite-minimum` quality gates | Narrower scope than the full repo path; no frontend/backend/docsite behavior in scope |
| [Run from source](docs/guide/local-dev-auth-login.md) with `mise run dev` | You are contributing, debugging, or want the full repo surfaces together | Highest: source checkout + toolchain install + backend/frontend/docsite processes + auth setup | Full contributor surface, but also the heaviest path |

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
| Backend      | Python 3.13+ (FastAPI)               |
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
ugoite-minimum/     # Portable Rust core layer for embedding/WASM-focused use
  └─ src/
docs/
  ├─ guide/         # User-facing guides and operator workflows
  ├─ spec/          # Technical specifications (YAML + Markdown)
  ├─ tests/         # Documentation consistency tests
  └─ version/       # Versioned roadmap YAML + release metadata
e2e/                # End-to-end tests (Bun)
```

---

## Documentation Map

Use **Start Here** above for the newcomer path. This section only lists the
additional references you usually open after that first choice.

### Operational guides

- [Backend Healthcheck](docs/guide/backend-healthcheck.md) - Quick backend readiness check
- [Environment Matrix](docs/guide/env-matrix.md) - Runtime variables and which surface consumes them

### Design and implementation references

- [Architecture Overview](docs/spec/architecture/overview.md) - System design
- [REST API Reference](docs/spec/api/rest.md) - Backend HTTP contract
- [MCP Reference](docs/spec/api/mcp.md) - Current resource-first MCP surface

### Release planning
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

The repository root `mise.toml` is the contributor-facing source of truth for
managed tool versions across both supported setup paths. Use that shared
toolchain story first, then treat package README prerequisites as workflow notes
on top of the same managed environment.

For the full contributor workflow around specs, REQ traceability, docsite
navigation wiring, and CI-parity checks, see
[Contributor Workflow](CONTRIBUTING.md).

Choose the contributor setup path that matches your machine:

| Path | Choose it when | What it handles for you |
| --- | --- | --- |
| Host-managed toolchain | You already want the repo toolchain on your machine or you are not using VS Code/Codespaces | You run `mise run setup` yourself to install dependencies and `uvx pre-commit install`, then continue with `mise run dev`. |
| Devcontainer / GitHub Codespaces | You want a reproducible VS Code/Codespaces workspace or do not want to install the full toolchain on your host | `.devcontainer/devcontainer.json` preinstalls `mise`, `gh`, `oathtool`, then runs `mise install`, `mise run setup`, and `npx playwright install --with-deps chromium` for you. |

Install dependencies and repository pre-commit hooks:

```bash
mise run setup
```

The setup task also runs `uvx pre-commit install` so local commits use the same
hook chain as CI by default.

The devcontainer path runs that same bootstrap for you during container
creation, so both contributor setups land on the same local commands and hooks.

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
canonical auth-mode reference plus the step-by-step `mise run dev` workflow,
including the explicit `/login` browser flow, refreshing the local login
context, supported auth modes, and the `dev:backend`, `dev:frontend`, or
`dev:docsite` shortcuts when needed. See
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

## Devcontainer / GitHub Codespaces vs Docker Compose (deployment)

Important: this repo provides two distinct container-based workflows:

- Devcontainer / GitHub Codespaces (development):
  - `.devcontainer/devcontainer.json` is the supported contributor container
    path. It preinstalls `mise`, `gh`, `oathtool`, then runs `mise install`,
    `mise run setup`, and `npx playwright install --with-deps chromium` for
    you.
  - Use the devcontainer when you want onboarding or day-to-day development in
    a reproducible VS Code/Codespaces workspace without installing the full
    toolchain on your host.

- Docker Compose (deployment / CI):
  - `docker-compose.yaml` is for containerized deployments or CI systems. If you use this for production, verify commands and configuration (e.g., remove `--reload` or `bun dev` and opt for production servers and built frontend assets).

These two environments are separate and intended for different uses—use the
devcontainer for contributor development and Docker Compose for deployments.

---

## Container Quick Start (published images)

Start here if you want the quickest way to try a published Ugoite release.
This path uses the shipped release compose file plus published GHCR images and
does not require cloning the repository or building images from source.

Prepare the compose file and an `.env` file with install-specific auth values,
then pull and start the published stack:

```bash
mkdir -p ugoite-release
cd ugoite-release
curl -fsSLO "https://github.com/ugoite/ugoite/releases/latest/download/docker-compose.release.yaml"
python3 - <<PY > .env
import secrets

demo_mode = "mock-oauth"
signing_kid = "release-compose-local-v1"
signing_secret = secrets.token_urlsafe(32)
proxy_token = secrets.token_urlsafe(32)

print("UGOITE_VERSION=stable")
print("UGOITE_SPACES_DIR=./spaces")
print("UGOITE_FRONTEND_PORT=3000")
print("UGOITE_BACKEND_PORT=8000")
print(f"UGOITE_DEV_AUTH_MODE={demo_mode}")
print("UGOITE_DEV_USER_ID=dev-local-user")
print(f"UGOITE_DEV_SIGNING_KID={signing_kid}")
print(f"UGOITE_DEV_SIGNING_SECRET={signing_secret}")
print(f"UGOITE_AUTH_BEARER_SECRETS={signing_kid}:{signing_secret}")
print(f"UGOITE_AUTH_BEARER_ACTIVE_KIDS={signing_kid}")
print(f"UGOITE_DEV_AUTH_PROXY_TOKEN={proxy_token}")
PY
mkdir -p ./spaces
docker compose -f docker-compose.release.yaml pull
docker compose -f docker-compose.release.yaml up -d
```

Then open `http://localhost:3000/login`, click **Continue with Local Demo Login**,
and you will land on `/spaces`. The shipped compose file bootstraps the `default`
space at startup so the first browser and CLI session both have a ready
workspace. The shipped manifest itself stays on the safer `passkey-totp`
default; the example above explicitly opts into loopback-only `mock-oauth` with
install-specific secrets. For more background on the explicit browser login
flow, see [Local Development Authentication and Login](docs/guide/local-dev-auth-login.md).

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
| `UGOITE_DEV_AUTH_MODE`        | `passkey-totp`               | Shipped auth-mode default; set it to `mock-oauth` only for an explicit local demo flow                                                                                               |
| `UGOITE_DEV_USER_ID`          | `required`                   | Username/user id for the explicit login flow you enable; the quick-start example sets `dev-local-user`                                                                               |
| `UGOITE_DEV_SIGNING_KID`      | `release-compose-local-v1`   | Key id paired with the install-specific bearer signing material                                                                                                                       |
| `UGOITE_DEV_SIGNING_SECRET`   | `required 32-byte random secret` | Secret used to mint dev bearer tokens for this install                                                                                                                                 |
| `UGOITE_AUTH_BEARER_SECRETS`  | `required 32-byte random secret` | Bearer verification secret set accepted by the backend                                                                                                                                 |
| `UGOITE_AUTH_BEARER_ACTIVE_KIDS` | `release-compose-local-v1` | Active bearer-token key ids accepted by the backend; keep this aligned with the signing key ids you expose for this install                                                          |
| `UGOITE_DEV_AUTH_PROXY_TOKEN` | `required 32-byte random secret` | Shared token wiring between the frontend proxy and backend dev auth flow                                                                                                              |

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

Run the CI-aligned CLI coverage gate without the full repo suite:

```bash
mise run //ugoite-cli:test:coverage
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

Contributions welcome! Start with [Run from source](docs/guide/local-dev-auth-login.md)
for the canonical contributor workflow, or open the repo Devcontainer / GitHub
Codespaces path when you want the preloaded contributor environment before
continuing with `mise run dev` and `/login`. If you are using an AI coding
agent in this repository, also read
[AGENTS.md](AGENTS.md).

1. Check [open issues](https://github.com/ugoite/ugoite/issues) and [pull requests](https://github.com/ugoite/ugoite/pulls) for current work items
2. Open an issue to discuss larger changes
3. Submit PR with tests
