# CLI Guide

This guide explains how to run the Ugoite CLI inside the devcontainer.

## Install dependencies

From the repository root:

```bash
mise run //ugoite-cli:install
```

Alternatively, you can build directly in the CLI folder:

```bash
cd ugoite-cli
cargo build
```

## Run the CLI

From the repository root, run the Rust CLI with Cargo:

```bash
cargo run -q -p ugoite-cli -- --help
```

## Basic workflow

Create a local data directory and list spaces:

```bash
mkdir -p ./spaces
cargo run -q -p ugoite-cli -- space list --root ./spaces
```

Create a new space:

```bash
cargo run -q -p ugoite-cli -- create-space --root ./spaces demo
```

## Seed local sample data

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
UGOITE_SEED_SPACE_ID=ux-demo UGOITE_SEED_SCENARIO=supply-chain \
UGOITE_SEED_ENTRY_COUNT=25 UGOITE_SEED_VALUE=42 mise run seed
mise run seed:scenarios
```

If you prefer the underlying direct command, the task is just a thin wrapper
over the Rust CLI and keeps Cargo builds inside the shared `target/rust` cache:

```bash
bash scripts/dev-seed.sh --space-id cli-demo --scenario lab-qa --entry-count 10 --seed 7
CARGO_TARGET_DIR=target/rust cargo run -q -p ugoite-cli -- space sample-scenarios
```

## Notes

- In `core` mode, commands that touch local storage require an explicit
  `--root <LOCAL_ROOT>` flag. Use the same `./spaces` directory as the Docker
  Compose setup for consistency.
- If you use another directory, ensure it is writable and backed by local
  storage.

## Endpoint routing mode

CLI can run in three modes, and stores the selection in `~/.ugoite/cli-endpoints.json`.

- `core`: call `ugoite-core` directly (default)
- `backend`: call backend REST endpoints directly (e.g. `http://localhost:8000`)
- `api`: call the frontend-proxied API base (e.g. `http://localhost:3000/api`)

When mode is `backend` or `api`, remote commands accept a `SPACE_ID` (or the
shared `SPACE_ID_OR_PATH` argument for commands that also work in `core` mode)
and perform filesystem I/O on the remote server instead of the local machine.

```bash
# Show current setting
cargo run -q -p ugoite-cli -- config show

# Route commands to backend directly
cargo run -q -p ugoite-cli -- config set --mode backend --backend-url http://localhost:8000

# Route commands to API endpoint
cargo run -q -p ugoite-cli -- config set --mode api --api-url http://localhost:3000/api
```


## Auth profile commands

In `backend` / `api` modes, CLI and frontend share the same bearer token env convention:

- `UGOITE_AUTH_BEARER_TOKEN`
- `UGOITE_AUTH_API_KEY`

```bash
# Route auth commands to the backend
cargo run -q -p ugoite-cli -- config set --mode backend --backend-url http://localhost:8000

# Exchange username + 2FA for a bearer token in manual-totp mode
cargo run -q -p ugoite-cli -- auth login --username dev-local-user --totp-code 123456

# Or use the explicit mock OAuth login path
cargo run -q -p ugoite-cli -- auth login --mock-oauth

# Inspect active auth setup
cargo run -q -p ugoite-cli -- auth profile

# Print unset commands
cargo run -q -p ugoite-cli -- auth token-clear
```
