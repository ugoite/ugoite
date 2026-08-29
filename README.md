# Ugoite

Ugoite is a private, portable Knowledge Space for humans and AI. Knowledge
stays in an operator-owned Space; a server, browser session, model provider, or
generated experience does not become its owner.

> Knowledge persists. Work may disappear. Knowledge can become tools.

The repository-level [`docs/`](docs/index.md) directory is the single source of
truth for product, operator, architecture, and specification documentation. The
Starlight site renders those files directly; this README intentionally stays
small so it cannot drift into a second manual.

## What makes Ugoite different

- **Own your Knowledge.** Knowledge lives in your Space and remains portable
  across the runtimes that work with it.
- **Let humans and agents work with it.** The CLI, browser, MCP, and Konase
  operate on the same Space-owned Knowledge through shared semantics.
- **Turn Knowledge into tools.** The same Knowledge can eventually become
  purpose-built views and task-specific applications without moving into a
  second system of record.

## Current reality

- CLI core mode is the shipped direct-local path for an operator-owned Space.
- The browser is currently server-backed. Browser-local persistence and
  optional synchronization are planned, not shipped.
- Konase currently provides a portable client-side Work/Job control plane.
  Temporary context, model interaction, and execution progress are Work, not
  durable Knowledge authority.
- Knowledge-to-tools is a North Star capability. v0.1 does not ship a View DSL,
  renderer, low-code editor, general application builder, or arbitrary code
  runtime.

## Start / Operate / Develop / Verify

- **Operate:** [Container quick start](docs/guide/start/container-quickstart.md)
  and [operations](docs/guide/operate/server/operations.md)
- **Develop:** [Local development](docs/guide/develop/local-dev-auth-login.md)
- **Automate:** [CLI guide](docs/guide/automate/cli.md),
  [REST and OpenAPI](docs/spec/api/rest.md), and the current
  [MCP surface](docs/spec/api/mcp.md)
- **Understand:** [Architecture](docs/architecture/index.md)
- **Verify:** [Executable specification](docs/spec/index.md)

For repository development, install [mise](https://mise.jdx.dev/) and run:

```bash
mise run setup
mise run dev
```

Validation is centralized at the repository root:

```bash
mise run fmt
mise run lint
mise run check
mise run test
mise run build
mise run package
mise run verify
mise run e2e:smoke
```

Local CI-parity entrypoints:

- `mise run test`: the canonical non-E2E Rust, tooling, frontend-coverage, and docsite-coverage suite
- `mise run ci`: formatting, lint, source checks, and the canonical test suite
- `mise run ci:artifacts`: canonical build/package/verify tasks, focused docsite navigation, E2E smoke/asset acceptance, and release validation
- `mise run ci:merge`: the complete local merge gate (`ci` plus `ci:artifacts`)
- `mise run ci:release`: `ci:merge` plus the full E2E suite

Tasks are defined at the repository root; package-scoped `mise run //...`
commands are not supported. `build:*` tasks may be skipped locally when declared
outputs are newer than their inputs, but test tasks remain authoritative and are
never satisfied by cached success markers.

## Run with Docker Compose

Source build:

```bash
export UGOITE_NODE_SECRET_KEY="$(head -c 32 /dev/urandom | base64)"
docker compose up --build -d
docker compose port ugoite 8000
```

Release image:

```bash
export UGOITE_VERSION=<release-tag>
export UGOITE_NODE_SECRET_KEY="$(head -c 32 /dev/urandom | base64)"
docker compose -f docker-compose.release.yaml up -d
docker compose -f docker-compose.release.yaml logs ugoite
```

Both configurations run one image and mount the default local Space storage and
Node control store at `/data`. The supplied image runs as a non-root user.
`UGOITE_NODE_SECRET_KEY` is an external recovery input and is not copied by a
`/data` snapshot; `UGOITE_NODE_CONTROL_URI` can place the Node control store in
another backend. Preserve each configured Space prefix, the control-store
prefix, and the node secret separately as needed.

## CLI examples

Core mode:

```bash
ugoite config set --mode core
ugoite space list /path/to/workspace
ugoite space create /path/to/workspace/spaces/demo
ugoite entry list /path/to/workspace/spaces/demo
ugoite query /path/to/workspace/spaces/demo --sql "SELECT id, title FROM note LIMIT 10"
```

Server-backed mode:

```bash
ugoite config set --mode backend --backend-url http://127.0.0.1:8000
ugoite space list
```

`ugoite auth login` uses browser-approved device authentication with a fresh
P-256 key and DPoP-bound credentials. Owner-approved Space access recovery is
available at the browser recovery route. Account Self-Recovery is available at
`/recover/account` after explicitly enrolling a recovery-only TOTP. Invitation-
gated OIDC login, account creation, and external identity linking are supported;
administrator recovery, agent principals, and remote CLI asset upload remain
future scope.

## API and documentation

- Human API summary: [`docs/spec/api/rest.md`](docs/spec/api/rest.md)
- Generated OpenAPI snapshot:
  [`docs/spec/api/openapi.yaml`](docs/spec/api/openapi.yaml)
- MCP resource surface: [`docs/spec/api/mcp.md`](docs/spec/api/mcp.md)
- Architecture:
  [`docs/architecture/principles/north-star.md`](docs/architecture/principles/north-star.md)
- Operator guides: [`docs/guide`](docs/guide)
- Executable specification registry: [`docs/spec`](docs/spec)

The server-generated `/openapi.json` is authoritative.
`cargo run -p xtask -- openapi-check` verifies the checked-in YAML snapshot.

## Distribution safety

Prefer package-manager or release-archive installation. The optional npm
installer verifies release checksums before installing a binary. Do not present
`curl | sh` as the recommended installation path.

`version.txt` is the canonical prepared product version. Run `mise run version:sync`
and `mise run version:check` for its Cargo, npm, Helm, and lockfile projections.
Ordinary pushes do not update release metadata. An
operator prepares a compatible or breaking version, merges that release PR,
dispatches `.github/workflows/release-candidate.yml`, and promotes the exact
verified candidate with `.github/workflows/release-publish.yml`. Promotion does
not rebuild or repackage candidate bytes.

## License

MIT
