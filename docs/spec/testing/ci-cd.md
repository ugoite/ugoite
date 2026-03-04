# CI/CD Pipeline

## GitHub Actions Workflows

| Workflow | File | Triggers | Purpose |
|----------|------|----------|---------|
| Python CI | `.github/workflows/python-ci.yml` | Push, PR | Lint (ruff), type check, pytest |
| YAML Workflow CI | `.github/workflows/yaml-workflow-ci.yml` | Push, PR | Lint (yamllint + actionlint) |
| Frontend CI | `.github/workflows/frontend-ci.yml` | Push, PR | Lint (biome) |
| E2E Tests | `.github/workflows/e2e-ci.yml` | Push, PR | Full E2E with live servers |
| Docker Build CI | `.github/workflows/docker-build-ci.yml` | Push, PR | Build backend/frontend images and validate compose |
| Devcontainer CI | `.github/workflows/devcontainer-ci.yml` | Push, PR | Build/smoke devcontainer with authenticated pulls and cache |
| SBOM CI | `.github/workflows/sbom-ci.yml` | Push, PR, merge queue | Generate CycloneDX SBOMs, sign/attest, and run vulnerability gate |
| Commitlint CI | `.github/workflows/commitlint-ci.yml` | PR, merge queue | Enforce Conventional Commits |
| Release CI | `.github/workflows/release-ci.yml` | Push on `main` | Automated semantic-release |

## Python CI

```yaml
jobs:
  lint:
    - ruff format --check .
    - ruff check .
  type-check:
    - cd backend && uv run ty check .
    - cd ugoite-cli && uv run ty check .
  test:
    - cd backend && uv run pytest
    - cd ugoite-cli && uv run pytest
```

## YAML/Workflow CI

```yaml
jobs:
  ci:
    - yamllint (repo YAML/config checks)
    - actionlint (GitHub Actions workflow lint)
```

## Frontend CI

```yaml
jobs:
  lint:
    - cd frontend && biome ci .
  test:
    - cd frontend && bun test
```

## E2E CI

```yaml
jobs:
  e2e:
    - Start backend (background)
    - Start frontend (background)
    - Wait for servers
    - cd e2e && npm run test
    timeout: 30 minutes
```

## Devcontainer CI

```yaml
jobs:
  devcontainer-build-smoke:
    - docker/setup-buildx-action (enables type=gha cache driver)
    - docker/login-action (ghcr.io, GITHUB_TOKEN)
    - devcontainers/ci build + smoke command
    - Run smoke command: gh/mise/bash versions
```

## SBOM and Supply Chain CI

```yaml
jobs:
  sbom-supply-chain:
    - Build backend/frontend Docker images
    - Generate CycloneDX SBOMs with Syft (rust/python/node/bun/docker)
    - Sign and verify SBOM artifacts with Cosign keyless flow
    - Emit artifact provenance attestation
    - Run Grype vulnerability checks (critical gate on source SBOMs)
```

## Pre-commit Hooks

Install and enable:
```bash
uvx pre-commit install
uvx pre-commit run --all-files
```

Hooks configured in `.pre-commit-config.yaml`:
- **Ruff**: Auto-formats and lints Python
- **Yamllint**: Validates YAML syntax/style on committed YAML files
- **Actionlint**: Validates `.github/workflows/*` syntax and workflow semantics
- **Ty**: Type checks Python projects

Conventional Commit enforcement (local):

```bash
npm install
npm run prepare
```

This enables Husky `commit-msg` hook and runs `commitlint` before commit is accepted.

## Release Process

1. **Conventional Commits** are required locally (Husky + Commitlint) and in CI (`commitlint-ci`).
2. **Static checks and tests** must pass through existing CI workflows and `All Tests Status`.
3. **Changesets** track monorepo change intent (`.changeset/*.md`).
4. **Release CI** runs automatically on pushes to `main`.
5. **semantic-release** computes the release from commit history, tags, and publishes GitHub release notes.

## Environment Variables

### CI Environment

| Variable | Purpose |
|----------|---------|
| `UGOITE_ROOT` | Data directory path |
| `UGOITE_ALLOW_REMOTE` | Enable remote connections |
| `BACKEND_URL` | Backend URL for frontend proxy |

### Secrets

| Secret | Purpose |
|--------|---------|
| `DOCKER_USERNAME` | Container registry auth |
| `DOCKER_PASSWORD` | Container registry auth |

## Local CI Verification

Before pushing, run the same checks as CI:

```bash
# Python
uvx ruff format --check .
uvx ruff check .
cd backend && uv run ty check . && uv run pytest
cd ugoite-cli && uv run ty check . && uv run pytest

# Frontend
cd frontend && biome ci . && bun test

# E2E (requires servers running)
mise run e2e

# Conventional commits + release metadata
npm run commitlint:range
npm run changeset:status
```

Or use pre-commit:
```bash
uvx pre-commit run --all-files
```
