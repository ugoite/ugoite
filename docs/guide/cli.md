# CLI Guide

This guide explains how to run the Ugoite CLI inside the devcontainer.

## Install dependencies

From the repository root:

```bash
mise run //ugoite-cli:install
```

Alternatively, you can install directly in the CLI folder:

```bash
cd ugoite-cli
uv sync
```

## Run the CLI

The CLI is exposed as the `ugoite` command via `uv run`.

```bash
uv run ugoite --help
```

## Basic workflow

Create a local data directory and list spaces:

```bash
mkdir -p ./spaces
uv run ugoite space list ./spaces
```

Create a new space:

```bash
uv run ugoite create-space ./spaces demo
```

## Notes

- The CLI expects a root path argument for most commands. Use the same `./spaces`
  directory as the Docker Compose setup for consistency.
- If you use another directory, ensure it is writable and backed by local
  storage.
