# Container Quick Start

This guide covers the official release image archives attached to each GitHub
Release. Use it when you want to run a tagged Ugoite release without cloning
the repository and rebuilding images locally.

For local development from source, keep using
[Docker Compose Guide](docker-compose.md).

## Published release assets

Each release publishes these container quick-start assets:

- `docker-compose.release.yaml`
- `ugoite-backend-v<version>.docker.tar.gz`
- `ugoite-frontend-v<version>.docker.tar.gz`

Loading the archives restores these canonical image names locally so the
Compose file works unchanged:

- `ghcr.io/ugoite/ugoite/backend`
- `ghcr.io/ugoite/ugoite/frontend`

Each release also pushes the same images to GHCR with these tag conventions for
authenticated pulls:

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

Download an exact release and start it:

```bash
UGOITE_VERSION=0.0.1
curl -fsSLO "https://github.com/ugoite/ugoite/releases/download/v${UGOITE_VERSION}/docker-compose.release.yaml"
curl -fsSLO "https://github.com/ugoite/ugoite/releases/download/v${UGOITE_VERSION}/ugoite-backend-v${UGOITE_VERSION}.docker.tar.gz"
curl -fsSLO "https://github.com/ugoite/ugoite/releases/download/v${UGOITE_VERSION}/ugoite-frontend-v${UGOITE_VERSION}.docker.tar.gz"
gzip -dc "ugoite-backend-v${UGOITE_VERSION}.docker.tar.gz" | docker load
gzip -dc "ugoite-frontend-v${UGOITE_VERSION}.docker.tar.gz" | docker load
UGOITE_VERSION="$UGOITE_VERSION" docker compose -f docker-compose.release.yaml up -d
```

Then open:

- Frontend UI: http://localhost:3000
- Backend API: http://localhost:8000

To stop the stack:

```bash
UGOITE_VERSION=0.0.1 docker compose -f docker-compose.release.yaml down --remove-orphans
```

The downloaded archives preserve the exact tags
`ghcr.io/ugoite/ugoite/backend:${UGOITE_VERSION}` and
`ghcr.io/ugoite/ugoite/frontend:${UGOITE_VERSION}`, so no Compose edits are
required after `docker load`.

Use exact release versions such as `0.0.1`, `0.0.1-beta.7`, or
`0.0.1-alpha.3` when downloading release assets.

## Authenticated GHCR pulls

If you already have GHCR package access, you can pull the canonical image tags
directly instead of downloading the release archives first.

You can replace `0.0.1` with:

- an exact stable release such as `0.0.1`
- `latest` or `stable` for the newest stable channel
- `alpha` or `beta` for the newest prerelease channel alias

```bash
docker pull ghcr.io/ugoite/ugoite/backend:0.0.1
docker pull ghcr.io/ugoite/ugoite/frontend:0.0.1
```

## Notes

- The release compose file keeps data on the host under `./spaces` to preserve
  the local-first storage model.
- The frontend container talks to the backend through the Compose network via
  `http://backend:8000`.
- Release image archives are attached to every GitHub Release so the public
  quick start does not depend on GHCR package visibility.
- If you want source-mounted development containers instead, use
  `docker-compose.yaml` and build locally.
