---
title: 'Docker Compose'
---

- `docker-compose.yaml` builds the current source and publishes a random loopback port.
- `docker-compose.release.yaml` runs `ghcr.io/ugoite/ugoite:${UGOITE_VERSION}` at `${UGOITE_PORT:-8000}`.

Both files define one `ugoite` service and mount host Space storage at `/data`.

```bash
# Source build
docker compose up --build -d
docker compose port ugoite 8000

# Release image
export UGOITE_VERSION=<release-tag>
export UGOITE_BOOTSTRAP_TOKEN="$(openssl rand -hex 32)"
docker compose -f docker-compose.release.yaml up -d
```

The current deployment is not split into separate frontend and backend images.
