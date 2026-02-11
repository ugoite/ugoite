# Docker Compose Guide

This guide describes how to run Ugoite with Docker Compose for local development.
If you are working inside the devcontainer and Docker is not available, use the
local dev workflow described in [README.md](../../README.md) instead.

## Prerequisites

- Docker Engine with Compose v2 (`docker compose`)
- Optional: enough disk space for the `spaces/` data directory

## Start the stack

```bash
docker compose up --build
```

The stack exposes:

- Backend API: http://localhost:8000
- Frontend UI: http://localhost:3000

The backend persists data in `./spaces` on the host. You can safely remove the
folder to reset local data.

## Verify status and logs

```bash
docker compose ps
```

```bash
docker compose logs -f backend
```

```bash
docker compose logs -f frontend
```

## Stop the stack

```bash
docker compose down --remove-orphans
```

## Reset local data (optional)

```bash
rm -rf ./spaces
```

## Notes

- The backend container enables remote access internally so the frontend can
  reach it across the Compose network.
- If you prefer to run services directly on the host, use the `mise` tasks
  instead of Docker Compose.
