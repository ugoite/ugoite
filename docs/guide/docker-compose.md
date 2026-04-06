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
export UGOITE_DEV_SIGNING_SECRET="$(openssl rand -hex 32)"
export UGOITE_DEV_AUTH_PROXY_TOKEN="$(openssl rand -hex 32)"
docker compose up --build
```

The stack exposes:

- Backend API: http://localhost:8000
- Frontend UI: http://localhost:3000

The backend persists data in `./spaces` on the host. You can safely remove the
folder to reset local data.

The source Compose file expects unique values for
`UGOITE_DEV_SIGNING_SECRET` and `UGOITE_DEV_AUTH_PROXY_TOKEN` before startup.
The backend derives `UGOITE_AUTH_BEARER_SECRETS` and
`UGOITE_AUTH_BEARER_ACTIVE_KIDS` from that signing material automatically, so
you only need to export the signing secret and proxy token once per local stack.

The shipped Compose file enables the explicit local demo login mode
(`mock-oauth`). On startup the backend bootstraps the configured
`UGOITE_DEV_USER_ID` into the reserved `admin-space`, so that user becomes the
local admin who can create new spaces after signing in at
`http://localhost:3000/login`.

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

If the stack fails before login, the browser stays blank, or the frontend and
backend cannot reach each other, continue with
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
- The shipped Compose file binds both published ports to `127.0.0.1`, so the
  browser-facing stack stays local-only even though the backend still accepts
  the frontend container's internal network hop.
- The configured `UGOITE_DEV_USER_ID` becomes the local `admin-space` admin for
  this source-based Compose stack.
- If you want the canonical contributor workflow, follow
  [Local Development Authentication and Login](local-dev-auth-login.md) and use
  `mise run dev` instead of Docker Compose.
