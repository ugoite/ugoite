# Docker Compose Guide

This guide describes an alternative way to run Ugoite from source when you
specifically want Docker Compose instead of the canonical human contributor
path.
If you want the default contributor workflow, start with
[Local Development Authentication and Login](local-dev-auth-login.md) and
`mise run dev` instead.

If you want pre-built release images from GHCR instead of local builds, use
[Container Quick Start](container-quickstart.md).

## Prerequisites

- Docker Engine with Compose v2 (`docker compose`)
- Optional: enough disk space for the `spaces/` data directory

## Start the stack

```bash
docker compose up --build
```

The stack exposes:

- Browser UI login: http://127.0.0.1:8000/login
- Backend API: http://127.0.0.1:8000/api

The server persists data in `./spaces` on the host. You can safely remove the
folder to reset local data.

The shipped Compose file enables the explicit local demo login mode
(`mock-oauth`). On startup the backend bootstraps the configured
`UGOITE_DEV_USER_ID` into the reserved `admin-space`, so that user becomes the
local admin who can create new spaces after signing in at
`http://127.0.0.1:8000/login`.

## Verify status and logs

```bash
docker compose ps
```

```bash
docker compose logs -f ugoite
```

If the stack fails before login or the browser stays blank, continue with
[Compose Startup and Connectivity Troubleshooting](troubleshooting-compose-startup.md).

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
- The source Compose stack binds both published ports to `127.0.0.1` by
  default, so the mock-oauth browser flow stays local-only unless you
  intentionally widen the `ports:` mappings.
- The configured `UGOITE_DEV_USER_ID` becomes the local `admin-space` admin for
  this source-based Compose stack.
- If you want the canonical contributor workflow, follow
  [Local Development Authentication and Login](local-dev-auth-login.md) and use
  `mise run dev` instead of Docker Compose.
