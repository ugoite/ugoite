---
name: ugoite-release
description: Use when touching release, packaging, Docker, Helm, or install/update flows.
---

# Ugoite Release

Use this skill for anything that changes how Ugoite is packaged, published,
installed, or deployed.

## Read first

- `docs/guide/docker-compose.md`
- `docs/guide/container-quickstart.md`
- `docs/guide/helm-chart.md`
- `docs/guide/cli.md`
- `packages/ugoite/README.md`
- `.github/workflows/release-*.yml`
- `.github/workflows/docker-*.yml`

## Release rules

- Keep version pins and SHA pins explicit.
- Do not loosen reproducibility just to make a build pass.
- Treat generated artifacts, installers, and release scripts as part of the
  contract.
- When changing packaging or install behavior, verify the published path and
  the source path separately.
- If a release change affects quickstart or install docs, update the matching
  guide in the same change.

## Validation

- `bash scripts/check-root-artifact-hygiene.sh`
- `docker compose -f docker-compose.yaml config`
- `docker compose -f docker-compose.release.yaml config`
- `bash scripts/verify-release-cli-quickstart.sh` when CLI release behavior
  changes
- `bash scripts/verify-release-container-quickstart.sh` when browser release
  behavior changes

