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

In `backend` / `api` modes, CLI and frontend share the same credential env conventions:

- `UGOITE_AUTH_BEARER_TOKEN`
- `UGOITE_AUTH_API_KEY`

```bash
# Print export commands for current shell
cargo run -q -p ugoite-cli -- auth login --bearer-token TOKEN_VALUE

# Inspect active auth setup
cargo run -q -p ugoite-cli -- auth profile

# Print unset commands
cargo run -q -p ugoite-cli -- auth token-clear
```
