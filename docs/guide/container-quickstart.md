# Container Quick Start

This guide covers the official release images published to GitHub Container
Registry (GHCR). Use it when you want to run a tagged Ugoite release without
cloning the repository and rebuilding images locally.

For local development from source, keep using
[Docker Compose Guide](docker-compose.md).

## Published images

Each release publishes these images:

- `ghcr.io/ugoite/ugoite/backend`
- `ghcr.io/ugoite/ugoite/frontend`

Tag conventions:

- stable releases publish the exact SemVer tag, plus `latest` and `stable`
- alpha releases publish the exact prerelease SemVer tag, plus `alpha`
- beta releases publish the exact prerelease SemVer tag, plus `beta`

Examples:

- `ghcr.io/ugoite/ugoite/backend:0.0.1`
- `ghcr.io/ugoite/ugoite/backend:latest`
- `ghcr.io/ugoite/ugoite/frontend:0.0.1-beta.2`
- `ghcr.io/ugoite/ugoite/frontend:beta`

## Quick start with Docker Compose

Create the local-first data directory:

```bash
mkdir -p spaces
```

Start a published release:

```bash
UGOITE_VERSION=0.0.1 docker compose -f docker-compose.release.yaml pull
UGOITE_VERSION=0.0.1 docker compose -f docker-compose.release.yaml up -d
```

Then open:

- Frontend UI: http://localhost:3000
- Backend API: http://localhost:8000

To stop the stack:

```bash
UGOITE_VERSION=0.0.1 docker compose -f docker-compose.release.yaml down --remove-orphans
```

You can replace `0.0.1` with:

- an exact stable release such as `0.0.1`
- `latest` or `stable` for the newest stable channel
- `alpha` or `beta` for the newest prerelease channel alias

## Pull images directly

If you want to inspect or mirror the release artifacts first:

```bash
docker pull ghcr.io/ugoite/ugoite/backend:0.0.1
docker pull ghcr.io/ugoite/ugoite/frontend:0.0.1
```

## Notes

- The release compose file keeps data on the host under `./spaces` to preserve
  the local-first storage model.
- The frontend container talks to the backend through the Compose network via
  `http://backend:8000`.
- If you want source-mounted development containers instead, use
  `docker-compose.yaml` and build locally.
