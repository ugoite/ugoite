# Ugoite

Ugoite is a local-first knowledge-space system built around operator-owned files.

> A private, portable knowledge space you can run with Docker, automate from the CLI, and keep on infrastructure you control.

## Principles

- **Low Cost** — one small Rust service and filesystem/object-storage-compatible data.
- **Easy** — a single container for the server, browser application, and CLI.
- **Freedom** — a Space is a portable directory tree; the operator owns the source of truth.

Entries and revisions are append-only files. SQLite indexes and SQL query sessions are derived data that can be rebuilt.

## Current product boundary

- The **CLI in core mode** directly opens a local workspace and is the current minimal local-first path.
- The **browser application is currently server-backed**. It uses the Rust REST API and does not yet persist a complete Space locally.
- Browser-local storage and optional synchronization are the North Star, while the server remains an optional relay/collaboration surface.
- Service-account CRUD and audit-log APIs are not shipped in this release.

## Repository layout

| Path | Responsibility |
|---|---|
| `crates/ugoite-domain` | transport- and storage-neutral domain types and validation |
| `crates/ugoite-api-client` | portable HTTP operation preparation and response decoding; no network I/O |
| `crates/ugoite-storage` | storage operator and filesystem/object-store access |
| `crates/ugoite-core` | application service over Spaces, entries, forms, assets, search, SQL, membership, and preferences |
| `crates/ugoite-server` | Axum REST/MCP server and static browser hosting |
| `crates/ugoite-cli` | local core-mode and remote API-mode command adapter |
| `crates/ugoite-wasm` | small JSON/C-ABI wrapper over portable Rust crates |
| `crates/xtask` | OpenAPI, architecture, and documentation consistency checks |
| `frontend` | SolidStart browser UI; currently server-backed |
| `docsite` | Astro documentation site |
| `e2e` | Playwright end-to-end tests |
| `packages/ugoite` | release-oriented npm installer source; it requires matching published CLI assets |

The Rust workspace has eight crates. Deno is the JavaScript/TypeScript task runner; there is no root npm/Bun development workflow.

## Development

Install [mise](https://mise.jdx.dev/), then:

```bash
mise run setup
mise run dev
```

For CLI-only iteration, use `mise run test:cli` before the full workspace test gate.
For the local dev stack URLs, browser auth flow, and docsite port, see [docs/guide/local-dev-auth-login.md](docs/guide/local-dev-auth-login.md).

Focused gates:

```bash
mise run fmt
mise run lint
mise run check
mise run test
mise run e2e:smoke
```

CI mappings:

- pull requests: `mise run ci`
- merge queue and pushes to `main`: `mise run ci:merge`
- local release candidate validation: `mise run ci:release`

Tasks are defined at the repository root; package-scoped `mise run //...` commands are not supported.

## Run with Docker Compose

Source build:

```bash
docker compose up --build -d
docker compose port ugoite 8000
```

Release image:

```bash
export UGOITE_VERSION=<release-tag>
export UGOITE_BOOTSTRAP_TOKEN="$(openssl rand -hex 32)"
docker compose -f docker-compose.release.yaml up -d
```

Both configurations run one image and mount the authoritative workspace at `/data`. The supplied image runs as a non-root user.

## CLI examples

Core mode:

```bash
ugoite config set --mode core
ugoite space list /path/to/workspace
ugoite space create /path/to/workspace/spaces/demo
ugoite entry list /path/to/workspace/spaces/demo
ugoite query /path/to/workspace/spaces/demo --sql "SELECT id, title FROM entries LIMIT 10"
```

Server-backed mode:

```bash
ugoite config set --mode backend --backend-url http://127.0.0.1:8000
eval "$(ugoite auth login --mock-oauth)"
ugoite space list
```

The username/TOTP CLI shape exists, but the Rust server currently rejects `/auth/login`; development uses `mock-oauth`.

## API and documentation

- Human API summary: [`docs/spec/api/rest.md`](docs/spec/api/rest.md)
- Generated OpenAPI snapshot: [`docs/spec/api/openapi.yaml`](docs/spec/api/openapi.yaml)
- MCP resource surface: [`docs/spec/api/mcp.md`](docs/spec/api/mcp.md)
- Architecture: [`docs/architecture/north-star.md`](docs/architecture/north-star.md)
- Operator guides: [`docs/guide`](docs/guide)
- Executable specification registry: [`docs/spec`](docs/spec)

The server-generated `/openapi.json` is authoritative. `cargo run -p xtask -- openapi-check` verifies the checked-in YAML snapshot.

## Distribution safety

Prefer package-manager or release-archive installation. The optional npm installer verifies release checksums before installing a binary. Do not present `curl | sh` as the recommended installation path.

## License

MIT
