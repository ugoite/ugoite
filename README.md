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

> **Browser path today:** the current browser route still needs a running
> server-backed browser stack and an explicit `/login` flow. If you want the
> lowest-setup-cost local-first path, start with the CLI in `core` mode.

### Choose your first step

- [Try the published release](docs/guide/container-quickstart.md) for the
  fastest browser-based evaluation path, while still running the browser stack
  with an explicit login step.
- [Run from source](docs/guide/local-dev-auth-login.md) when you want the current
  backend, frontend, and docsite together for full-stack evaluation or
  debugging; choose the repo Devcontainer / GitHub Codespaces path when you
  want the preloaded contributor environment (`mise`, `gh`, `oathtool`,
  `mise install`, `mise run setup`, and `deno task e2e:install:browsers`), or
  run `mise run setup` on your host when you already manage the
  toolchain yourself; both paths continue with `mise run dev`, followed by the
  explicit `/login` flow. If you are contributing to one surface at a time, use
  the [Contributor Workflow](CONTRIBUTING.md) after setup for targeted commands
  and validation.
  If you intentionally use the repo-root `docker compose up --build` path
  instead, export `UGOITE_DEV_SIGNING_SECRET` and
  `UGOITE_DEV_AUTH_PROXY_TOKEN` first with at least 32 characters of random
  secret material or startup will fail fast. The exact commands live in the
  [Docker Compose Guide](docs/guide/docker-compose.md).
- [Use the CLI](docs/guide/cli.md) for terminal-first workflows and scripting.

If you only need the portable Rust layer for WASM, embedding, or pure helper
work, start with [`ugoite-domain`](crates/ugoite-domain/README.md) and the portable
contributor notes in [Contributor Workflow](CONTRIBUTING.md).

### After your first step

- [Understand core concepts](docs/guide/concepts.md) when you want the mental
  model behind spaces, entries, forms, and search before you go deeper into
  auth or the specs.
  - [Understand auth and access](docs/guide/auth-overview.md) before rollout or
    scripting across the browser, CLI, and API.
  - [Create your first space, form, and entry](docs/guide/browser-first-entry.md)
    once `/login` succeeds and you want the exact `/spaces` -> form -> entry path.
- [Read design and source docs](docs/spec/index.md) when you need philosophy,
  requirements, APIs, or machine-readable specs.

For a brand-new browser space, use the
[Browser Walkthrough](docs/guide/browser-first-entry.md) when you want the
concrete first productive in-app sequence after login.

For the current runtime split, see [Control Surfaces](docs/architecture/control-surfaces.md).
The browser experience runs through the Rust `ugoite-server`, while the CLI can
use the direct `core` path for local filesystem-backed spaces. The long-term
direction is captured in [North Star](docs/architecture/north-star.md).

Auth defaults are now intentionally boring for local evaluation: `mise run dev`
and the checked-in Compose paths use the explicit local demo login mode
(`mock-oauth`) so `/login` and `/spaces` work without an unfinished
passkey/TOTP implementation. See the
[canonical auth reference](docs/guide/local-dev-auth-login.md) for the
implemented `mock-oauth` flow and the planned `passkey-totp` boundary.

### Which entry path should you choose?

| Path | Best when | Setup cost / requirements | Trade-off |
| --- | --- | --- | --- |
| [Try the published release](docs/guide/container-quickstart.md) | You want the fastest visual evaluation of the published browser experience | Medium: Docker + published image pulls + frontend/backend containers + explicit login | Browser-first, but still multi-service and login-gated |
| [Use the CLI](docs/guide/cli.md) in `core` mode | You want the lightest local-first workflow with direct filesystem access | Lowest: released CLI install + local filesystem path; no container stack required | Terminal-first experience; no browser UI or server-backed collaboration features |
| [Work on `ugoite-domain`](crates/ugoite-domain/README.md) | You are contributing portable Rust, WASM-oriented, or embedding-friendly logic without the full app stack | Medium: source checkout + `mise run setup`, then package-local `//ugoite-domain` quality gates | Narrower scope than the full repo path; no frontend/backend/docsite behavior in scope |
| [Run from source](docs/guide/local-dev-auth-login.md) with `mise run dev` | You want the current backend, frontend, and docsite together for source-based evaluation or full-stack debugging | Highest: source checkout + toolchain install + backend/frontend/docsite processes + auth setup | Full repo surface, but also the heaviest path |
| [Contributor Workflow](CONTRIBUTING.md) | You are changing docs, frontend, backend, or core and want the canonical setup plus targeted commands | Medium: source checkout + `mise run setup`; add only the surface-specific commands or services you need | Flexible contributor path, but cross-surface or auth changes may still need the full `mise run dev` stack |

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
| Frontend     | Deno + SolidStart + TailwindCSS      |
| Backend      | Rust (`ugoite-server`, Axum)          |
| Core         | Rust (`ugoite-domain` + `ugoite-core`) |
| Storage      | OpenDAL + Apache Iceberg             |
| AI Interface | MCP (resource-first integration today) |

---

## Directory Structure

```
frontend/           # SolidStart frontend
  ├─ src/
  └─ public/
crates/ugoite-cli/         # Command-line interface for power users
  └─ src/
crates/ugoite-core/        # Rust core logic + Python bindings
  └─ src/
crates/ugoite-server/      # Rust REST & MCP server
  └─ src/
ugoite-domain/     # Portable Rust core layer for embedding/WASM-focused use
  └─ src/
docs/
  ├─ guide/         # User-facing guides and operator workflows
  ├─ spec/          # Technical specifications (YAML + Markdown)
  ├─ tests/         # Documentation consistency tests
  └─ version/       # Versioned roadmap YAML + release metadata
e2e/                # End-to-end tests (Playwright via Deno)
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

Pin the current published package version when needed:

```bash
VERSION="$(npm view ugoite version)"
npm install -g "ugoite@${VERSION}"
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

Pin an exact release when you do not want the newest published build:

```bash
VERSION="$(npm view ugoite version)"
curl -fsSL https://raw.githubusercontent.com/ugoite/ugoite/main/scripts/install-ugoite-cli.sh | env UGOITE_VERSION="${VERSION}" bash
ugoite --help
```

Install an exact release with a platform-specific one-liner:

```bash
# Linux x86_64
VERSION="$(npm view ugoite version)"
curl -fsSL "https://github.com/ugoite/ugoite/releases/download/v${VERSION}/ugoite-v${VERSION}-x86_64-unknown-linux-gnu.install.sh" | bash

# Linux arm64
curl -fsSL "https://github.com/ugoite/ugoite/releases/download/v${VERSION}/ugoite-v${VERSION}-aarch64-unknown-linux-gnu.install.sh" | bash

# macOS x86_64
curl -fsSL "https://github.com/ugoite/ugoite/releases/download/v${VERSION}/ugoite-v${VERSION}-x86_64-apple-darwin.install.sh" | bash

# macOS arm64
curl -fsSL "https://github.com/ugoite/ugoite/releases/download/v${VERSION}/ugoite-v${VERSION}-aarch64-apple-darwin.install.sh" | bash
```

For contributor-oriented Cargo workflows, see [CLI Guide](docs/guide/cli.md).

## Setup & Development (Rust + Deno)

The repository root `mise.toml` is the only contributor-facing toolchain entry
point. It pins Rust and Deno; Cargo owns Rust work and Deno owns repository
TypeScript tasks. See
[the pre-release rearchitecture decision](docs/architecture/release-rearchitecture.md)
for the migration boundary and target state.

For the full contributor workflow around specs, REQ traceability, docsite
navigation wiring, and CI-parity checks, see
[Contributor Workflow](CONTRIBUTING.md).

Choose the contributor setup path that matches your machine:

| Path | Choose it when | What it handles for you |
| --- | --- | --- |
| Host-managed toolchain | You already want the repo toolchain on your machine or you are not using VS Code/Codespaces | You run `mise run setup` yourself to install dependencies plus the fast pre-commit hook chain and the heavier pre-push coverage hook, then continue with `mise run dev`. |
| Devcontainer / GitHub Codespaces | You want a reproducible VS Code/Codespaces workspace or do not want to install the full toolchain on your host | `.devcontainer/devcontainer.json` preinstalls `mise`, `gh`, `oathtool`, then runs `mise install`, `mise run setup`, and `deno task e2e:install:browsers` for you. |

Install and cache the minimal development toolchain:

```bash
mise run setup
```

Git hooks are optional and are not installed by setup. The canonical quality
gates are `mise run ci`, `mise run ci:merge`, and `mise run ci:release`.

The devcontainer path runs that same bootstrap for you during container
creation, so both contributor setups land on the same local commands and hooks.

Start development (Rust server + frontend + docsite; `mock-oauth` is the default local auth mode):

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

Important: During source development the frontend dev server proxies `/api`
requests to the Rust backend at `BACKEND_URL` (default:
`http://localhost:8000`). The release Compose path serves the built frontend
and API from the same Rust server on port 8000.

Details:

Backend (dev) example:

```bash
cargo run -p ugoite-server
```

Frontend (dev) example:

```bash
cd frontend
deno task dev
```

---

## Devcontainer / GitHub Codespaces vs Docker Compose (deployment)

Important: this repo provides two distinct container-based workflows:

- Devcontainer / GitHub Codespaces (development):
  - `.devcontainer/devcontainer.json` is the supported contributor container
    path. It preinstalls `mise`, `gh`, `oathtool`, then runs `mise install`,
    `mise run setup`, and `deno task e2e:install:browsers` for
    you.
  - Use the devcontainer when you want onboarding or day-to-day development in
    a reproducible VS Code/Codespaces workspace without installing the full
    toolchain on your host.

- Docker Compose (deployment / CI):
  - `docker-compose.yaml` is for containerized deployments or CI systems. If you use this for production, verify commands and configuration and use production-built frontend assets.

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
signing_kid="release-compose-local-v1"
signing_secret="$(openssl rand -base64 32 | tr -d '\n')"
cat > .env <<EOF
UGOITE_VERSION=stable
UGOITE_SPACES_DIR=./spaces
UGOITE_PORT=8000
UGOITE_DEV_AUTH_MODE=mock-oauth
UGOITE_DEV_USER_ID=dev-local-user
UGOITE_DEV_SIGNING_KID=${signing_kid}
UGOITE_DEV_SIGNING_SECRET=${signing_secret}
UGOITE_AUTH_BEARER_SIGNING_SECRETS=${signing_kid}:${signing_secret}
UGOITE_AUTH_BEARER_ACTIVE_KIDS=${signing_kid}
EOF
mkdir -p ./spaces
docker compose -f docker-compose.release.yaml pull
docker compose -f docker-compose.release.yaml up -d
```

Then open `http://localhost:8000/login`, click **Continue with Local Demo Login**,
and you will land on `/spaces`. The shipped compose file bootstraps the
`default` space at startup so the first browser and CLI session both have a
ready workspace. The checked-in Compose defaults use loopback-oriented
`mock-oauth`; use install-specific secrets for any shared environment. For more
background on the explicit browser login
flow, see [Local Development Authentication and Login](docs/guide/local-dev-auth-login.md).

The compose file pulls the canonical release image name used by
`docker-compose.release.yaml`:

- `ghcr.io/ugoite/ugoite:${UGOITE_VERSION}`

Tag conventions:

- stable releases publish the exact SemVer tag plus `latest` and `stable`
- alpha releases publish the exact prerelease tag plus `alpha`
- beta releases publish the exact prerelease tag plus `beta`

### Environment Variables

| Variable                      | Default                      | Purpose                                                                                                                                                                               |
| ----------------------------- | ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `UGOITE_VERSION`              | `required`                   | Published image tag selector; set it to `stable` or `latest` for the newest stable release, `alpha` or `beta` for the newest prerelease channel, or an exact version to pin the stack |
| `UGOITE_SPACES_DIR`           | `./spaces`                   | Host path mounted into the container at `/data`                                                                                                                                       |
| `UGOITE_PORT`                 | `8000`                       | Host port that exposes the single Rust server, including the browser UI and `/api/*`                                                                                                  |
| `UGOITE_DEV_AUTH_MODE`        | `mock-oauth`                 | Shipped local auth-mode default for the current Rust server; `passkey-totp` remains planned until implemented end-to-end                                                             |
| `UGOITE_DEV_USER_ID`          | `required`                   | Username/user id for the explicit login flow you enable; the quick-start example sets `dev-local-user`                                                                               |
| `UGOITE_DEV_SIGNING_KID`      | `release-compose-local-v1`   | Key id paired with the install-specific bearer signing material                                                                                                                       |
| `UGOITE_DEV_SIGNING_SECRET`   | `required 32-byte random secret` | Secret used to mint dev bearer tokens for this install                                                                                                                                 |
| `UGOITE_AUTH_BEARER_SIGNING_SECRETS` | `required 32-byte random secret` | Bearer verification secret set accepted by the backend                                                                                                                                 |
| `UGOITE_AUTH_BEARER_ACTIVE_KIDS` | `release-compose-local-v1` | Active bearer-token key ids accepted by the backend; keep this aligned with the signing key ids you expose for this install                                                          |

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

For faster local smoke coverage:

```bash
mise run e2e:smoke
```

Where you can run this:

- Dev Container: everything needed to run tests is available; run `mise run test`.
- GitHub Actions `ci-required`: runs `mise run ci` for pull requests.
- Local (non-container): install the pinned `mise` tools with `mise run setup`,
  then run the commands above.

Frontend and docsite tests are included in `mise run test` through the root
Deno tasks.

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
