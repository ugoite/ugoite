# Ugoite

Ugoite is a local-first knowledge-space system built around operator-owned
files.

> A private, portable knowledge space you can run with Docker, automate from the
> CLI, and keep on infrastructure you control.

The repository-level [`docs/`](docs/index.md) directory is the single source of
truth for product, operator, architecture, and specification documentation. The
Starlight site renders those files directly; this README intentionally stays
small so it cannot drift into a second manual.

## Start

- [Container quick start](docs/guide/start/container-quickstart.md)
- [Local development](docs/guide/develop/local-dev-auth-login.md)
- [CLI guide](docs/guide/automate/cli.md)
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
ugoite auth login --actions read,create,update
ugoite space list
```

Approve the displayed device code from a Passkey-authenticated browser.

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

Release versions are synchronized across Cargo, the scoped GitHub Packages
installer, and Helm metadata. Pushes to `main` update Release Please metadata
only; merging the Release Please PR publishes versioned non-docsite artifacts
from `.github/workflows/release-publish.yml`.

## License

MIT
