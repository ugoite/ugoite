# Helm Chart Guide

The in-repo Helm chart is currently **not a supported release artifact**.

The published Docker and Compose path has converged on a single Rust server
image (`ghcr.io/ugoite/ugoite`) that serves both `/api/*` and the built browser
assets. The existing chart under `charts/ugoite/` now mirrors that single-image
shape, but it still needs a dedicated Kubernetes validation pass before it is
promoted into the supported release surface.

For the supported browser path, use
[Container Quick Start](container-quickstart.md). For source-based container
development, use [Docker Compose Guide](docker-compose.md).
