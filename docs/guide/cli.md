# CLI Guide

This guide explains the recommended release-install path for `ugoite`, then
covers the Cargo-based workflow contributors still use inside the repository.

## Install the released CLI (recommended)

Install the public `ugoite` npm bootstrap package:

```bash
npm install -g ugoite
ugoite-install
ugoite --help
```

Pin an exact package version when you want the matching published release:

```bash
npm install -g ugoite@0.1.0
ugoite-install
ugoite --help
```

The published package metadata lives in `packages/ugoite/package.json`, while
the repository root `package.json` stays private tooling for Husky/commitlint
and release automation.

If you prefer the direct shell bootstrap, install the latest stable release with
a one-liner:

```bash
curl -fsSL https://raw.githubusercontent.com/ugoite/ugoite/main/scripts/install-ugoite-cli.sh | bash
ugoite --help
```

Pin an exact version when you want a specific release:

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

The installer writes `ugoite` into `~/.local/bin` by default. Override the
install directory with `UGOITE_INSTALL_DIR=/custom/bin` when needed.

Supported release artifacts currently target:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

Release archives use predictable names such as
`ugoite-v0.1.0-x86_64-unknown-linux-gnu.tar.gz` plus a matching `.sha256`
checksum file. Matching one-liner installer assets use predictable names such as
`ugoite-v0.1.0-x86_64-unknown-linux-gnu.install.sh`.

## Build from source (contributors)

Install dependencies from the repository root:

```bash
mise run //ugoite-cli:install
```

Alternatively, build directly in the CLI folder:

```bash
cd ugoite-cli
cargo build
```

Run the CLI from source with Cargo:

```bash
cargo run -q -p ugoite-cli -- --help
```

## Basic workflow

If you installed a released binary, use `ugoite` directly:

```bash
mkdir -p ./spaces
ugoite space list ./spaces
ugoite space create ./spaces/demo
```

If you are actively developing inside the repository, you can swap those for
`cargo run -q -p ugoite-cli -- ...` instead.

## Next steps after your first command

`./spaces/demo` is now your local workspace example. A good next step is to add
one plain Markdown entry there:

```bash
ugoite entry create ./spaces/demo first-note --content '# First note'
ugoite entry get ./spaces/demo first-note
```

You do **not** need Forms or sample data before this first note. Read
[Core Concepts](concepts.md) next if you want the mental model for spaces,
entries, forms, and search, or jump to
[Endpoint routing mode](#endpoint-routing-mode) when you want the CLI to talk to
a backend or API instead of the local filesystem.

## Contributor-only shortcut: seed local sample data

If you installed a released CLI and only want the basic local workflow, you can
skip this section. `mise run seed` is a repository task for contributors and
repeatable demos, not a required first step for end users.

Use the root developer task when you want a quick local dataset without
remembering the lower-level CLI arguments:

```bash
mise run seed
```

The seed task creates `./spaces/dev-seed` by default, uses the
`renewable-ops` scenario, and loads 50 generated entries. To choose a
different target space, scenario, entry count, or deterministic RNG seed, set
the matching environment variables before running the task:

```bash
UGOITE_SEED_SPACE_ID=ux-demo UGOITE_SEED_SCENARIO=supply-chain UGOITE_SEED_ENTRY_COUNT=25 UGOITE_SEED_VALUE=42 mise run seed
mise run seed:scenarios
```

The seed flow prints a terminal progress bar while entries are generated. After
it finishes, confirm the seeded space:

```bash
cargo run -q -p ugoite-cli -- space list ./spaces
ls "./spaces/${UGOITE_SEED_SPACE_ID:-dev-seed}"
```

If you prefer the underlying direct command, the task is just a thin wrapper
over the Rust CLI and keeps Cargo builds inside the shared `target/rust` cache:

```bash
bash scripts/dev-seed.sh --space-id cli-demo --scenario lab-qa --entry-count 10 --seed 7
CARGO_TARGET_DIR=target/rust cargo run -q -p ugoite-cli -- space sample-data . cli-demo --scenario lab-qa --entry-count 10 --seed 7
CARGO_TARGET_DIR=target/rust cargo run -q -p ugoite-cli -- space sample-scenarios
```

## Notes

- In `core` mode, commands that target a specific space now take a positional
  `SPACE_ID_OR_PATH`. Pass the full local path such as
  `./spaces/dev-seed` or `/root/spaces/dev-seed`.
- `ugoite space list` takes the local root as a positional argument. Both
  `ugoite space list .` and `ugoite space list ./spaces` resolve to the same
  local workspace root.
- If `UGOITE_ROOT` is already exported for the local dev backend, `mise run seed`
  reuses that same root automatically so the seeded dataset stays visible to the
  local stack.
- If you use another directory, ensure it is writable and backed by local
  storage.

## Migration: core-mode space paths

Older examples used `--root <LOCAL_ROOT>` for visible `space` subcommands.
Those flows now use positional paths instead:

```bash
# Before
ugoite space create --root . demo
ugoite space get --root . demo
ugoite space patch --root . demo --name "Renamed"
ugoite space list --root .

# After
ugoite space create ./spaces/demo
ugoite space get ./spaces/demo
ugoite space patch ./spaces/demo --name "Renamed"
ugoite space list .
```

Backend and API modes still accept bare space IDs such as `demo`.

## Endpoint routing mode

CLI can run in three modes, and stores the selection in `~/.ugoite/cli-endpoints.json`.

- `core`: call `ugoite-core` directly (default)
- `backend`: call backend REST endpoints directly (e.g. `http://localhost:8000`)
- `api`: call the frontend-proxied API base (e.g. `http://localhost:3000/api`)

When mode is `backend` or `api`, remote commands accept a `SPACE_ID` (or the
shared `SPACE_ID_OR_PATH` argument for commands that also work in `core` mode)
and perform filesystem I/O on the remote server instead of the local machine.

```bash
# Show the saved JSON config
cargo run -q -p ugoite-cli -- config show

# Show the active mode in plain language
cargo run -q -p ugoite-cli -- config current

# Route commands to backend directly
cargo run -q -p ugoite-cli -- config set --mode backend --backend-url http://localhost:8000

# Route commands to API endpoint
cargo run -q -p ugoite-cli -- config set --mode api --api-url http://localhost:3000/api

# Return to direct local filesystem access
cargo run -q -p ugoite-cli -- config set --mode core
```

When you switch away from `core`, the CLI now prints a reminder that future
commands will run against the configured server or API instead of your local
filesystem. Use `ugoite config current` whenever you want a quick plain-language
summary of the active topology and the command to return to `core`.

## Auth profile commands

In `backend` / `api` modes, CLI and frontend share the same bearer token env convention:

- `UGOITE_AUTH_BEARER_TOKEN`
- `UGOITE_AUTH_API_KEY`

```bash
# Route auth commands to the backend
cargo run -q -p ugoite-cli -- config set --mode backend --backend-url http://localhost:8000

# Exchange username + 2FA for a bearer token in passkey-totp mode
cargo run -q -p ugoite-cli -- auth login --username dev-local-user --totp-code 123456

# Or use the explicit mock OAuth login path
cargo run -q -p ugoite-cli -- auth login --mock-oauth

# Inspect active auth setup, endpoint mode, and the next useful auth action
cargo run -q -p ugoite-cli -- auth profile

# Print unset commands
cargo run -q -p ugoite-cli -- auth token-clear
```

`ugoite auth profile` distinguishes `core` mode (no backend credential required)
from `backend` / `api` modes. In server-backed modes it tells you whether a
bearer token or API key is already present, and whether the next step is
`ugoite auth login` or `ugoite auth token-clear`.

When the backend runs inside Docker/Compose and you target its published
backend port directly, export the matching `UGOITE_DEV_AUTH_PROXY_TOKEN` value
before `auth login` so the explicit local-login request stays trusted across the
container boundary.
