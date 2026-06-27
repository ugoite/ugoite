# Ugoite

Ugoite is a local-first knowledge-space system built around operator-owned files.

> A private, portable knowledge space you can run with Docker, automate from the CLI, and keep on infrastructure you control.

The repository-level [`docs/`](docs/index.md) directory is the single source of truth for product, operator, architecture, and specification documentation. The Starlight site renders those files directly; this README intentionally stays small so it cannot drift into a second manual.

## Start

- [Container quick start](docs/guide/container-quickstart.md)
- [Local development](docs/guide/local-dev-auth-login.md)
- [CLI guide](docs/guide/cli.md)
- [Architecture](docs/architecture/index.md)
- [REST and OpenAPI](docs/spec/api/rest.md)
- [Executable specification](docs/spec/index.md)

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

- `mise run ci`: formatting, lint, source checks, and non-E2E tests
- `mise run ci:merge`: `ci`, canonical build/package/verify tasks, and E2E smoke
- `mise run ci:release`: `ci:merge` plus the full E2E suite

Tasks are defined at the repository root; package-scoped `mise run //...` commands are not supported. `build:*` tasks may be skipped locally when declared outputs are newer than their inputs, but test tasks remain authoritative and are never satisfied by cached success markers.

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
