---
title: 'Container quick start'
---

When a tagged image has been published, the single release image contains the Rust server, `ugoite` CLI, and compiled browser application. Versioned images are published to `ghcr.io/ugoite/ugoite:<version>`, stable releases additionally refresh the `stable` and `latest` aliases, and prereleases refresh only their matching `alpha` or `beta` alias.

```bash
export UGOITE_VERSION=<release-tag>
export UGOITE_BOOTSTRAP_TOKEN="$(openssl rand -hex 32)"
docker compose -f docker-compose.release.yaml up -d
curl --fail http://127.0.0.1:${UGOITE_PORT:-8000}/health
```

`${UGOITE_SPACES_DIR:-./spaces}` is mounted at `/data`, the authoritative workspace. The image runs as a non-root user.

The release Compose file defaults to development `mock-oauth`; configure an appropriate authentication mode before exposing the service outside a trusted local environment.
