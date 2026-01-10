# CI/CD Pipeline

## GitHub Actions Workflows

| Workflow | File | Triggers | Purpose |
|----------|------|----------|---------|
| Python CI | `.github/workflows/python-ci.yml` | Push, PR | Lint, type check, pytest |
| Frontend CI | `.github/workflows/frontend-ci.yml` | Push, PR | Lint (biome) |
| E2E Tests | `.github/workflows/e2e-ci.yml` | Push, PR | Full E2E with live servers |

## Python CI

```yaml
jobs:
  lint:
    - ruff format --check .
    - ruff check .
  type-check:
    - cd backend && uv run ty check .
    - cd ieapp-cli && uv run ty check .
  test:
    - cd backend && uv run pytest
    - cd ieapp-cli && uv run pytest
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
    - mise run sandbox:build
    - Start backend (background)
    - Start frontend (background)
    - Wait for servers
    - cd e2e && bun test
    timeout: 30 minutes
```

## Pre-commit Hooks

Install and enable:
```bash
uvx pre-commit install
uvx pre-commit run --all-files
```

Hooks configured in `.pre-commit-config.yaml`:
- **Ruff**: Auto-formats and lints Python
- **Ty**: Type checks Python projects

## Release Process

1. **Version Bump**: Update version in `pyproject.toml` files
2. **Changelog**: Update CHANGELOG.md
3. **Tag**: Create git tag `v{version}`
4. **Build**: CI builds Docker images and wheels
5. **Publish**: Push to registry (manual approval)

## Environment Variables

### CI Environment

| Variable | Purpose |
|----------|---------|
| `IEAPP_ROOT` | Data directory path |
| `IEAPP_ALLOW_REMOTE` | Enable remote connections |
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
cd ieapp-cli && uv run ty check . && uv run pytest

# Frontend
cd frontend && biome ci . && bun test

# E2E (requires servers running)
mise run e2e
```

Or use pre-commit:
```bash
uvx pre-commit run --all-files
```
