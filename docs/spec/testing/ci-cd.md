# CI/CD Pipeline

## GitHub Actions Workflows

| Workflow | File | Triggers | Purpose |
|----------|------|----------|---------|
| Python CI | `.github/workflows/python-ci.yml` | Push, PR | Lint, type check, pytest |
| Rust CI | `.github/workflows/rust-ci.yml` | Push, PR, merge queue | Core/CLI format, lint, test, and coverage |
| Frontend CI | `.github/workflows/frontend-ci.yml` | Push, PR | Lint (biome) |
| Docsite CI | `.github/workflows/docsite-ci.yml` | Push, PR | Lint, format check, typecheck, validation test |
| E2E Tests | `.github/workflows/e2e-ci.yml` | Push, PR | Full E2E with live servers |
| Docker Build CI | `.github/workflows/docker-build-ci.yml` | Push, PR | Build backend/frontend images and validate compose |
| Devcontainer CI | `.github/workflows/devcontainer-ci.yml` | Push/PR for devcontainer & setup inputs, merge queue | Build/smoke devcontainer with authenticated pulls and path-filtered setup contracts |
| SBOM CI | `.github/workflows/sbom-ci.yml` | Push, PR, merge queue | Generate CycloneDX SBOMs, sign/attest, and run vulnerability gate |
| Commitlint CI | `.github/workflows/commitlint-ci.yml` | PR, merge queue | Enforce Conventional Commits |
| PR Template Validation | `.github/workflows/pr-require-close-issue.yml` | PR body events via `pull_request_target` | Enforce required PR sections and accepted close/closes issue links |
| All Tests Status | `.github/workflows/all-tests-ci.yml` | Push on `main`, PR, merge queue | Aggregate code-quality workflow health while excluding release/publish automation |
| Release CI | `.github/workflows/release-ci.yml` | Push on `main` | Create/update release PR with release-please (no auto publish) |
| Release Publish | `.github/workflows/release-publish.yml` | Manual (`workflow_dispatch`) | Human-approved stable/alpha/beta GitHub release publish |

Backend image builds in Docker Build CI, E2E CI, and SBOM CI pass `ugoite-core`,
`ugoite-minimum`, and `ugoite-cli` as Buildx contexts so Rust path dependencies
resolve inside the container build.

## Python CI

```yaml
jobs:
  ci:
    - uvx ruff check --select ALL --ignore-noqa .
    - uvx ruff format --check .
    - cd backend && uv run ty check .
    - cd backend && uv run pytest -W error
    - uv run --with pytest --with pyyaml --with bashlex pytest docs/tests -W error
```

## Rust CI

```yaml
jobs:
  ci:
    - cd ugoite-core && uv run ty check .
    - cd ugoite-core && cargo fmt --check
    - cd ugoite-core && cargo clippy -- -D warnings
    - cd ugoite-core && cargo test --no-run
    - cd ugoite-core && uv run maturin develop
    - cd ugoite-core && uv run pytest -W error
    - cd ugoite-core && cargo llvm-cov --summary-only --fail-under-lines 45
    - cd ugoite-cli && cargo fmt --check
    - cd ugoite-cli && cargo clippy --no-default-features -- -D warnings
    - cd ugoite-cli && cargo test --no-default-features
```

## Frontend CI

```yaml
jobs:
  lint:
    - cd frontend && biome ci .
  test:
    - cd frontend && bun test
```

## Docsite CI

```yaml
jobs:
  ci:
    - cd docsite && bun run lint
    - cd docsite && bun run format:check
    - cd docsite && bun run typecheck
    - cd docsite && bun run test:validation
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
  version-consistency:
    - pytest docs/tests/test_guides.py::test_docs_req_ops_001_mise_versions_match_ci_pins
    - pytest docs/tests/test_guides.py::test_docs_req_ops_012_devcontainer_trigger_paths_cover_inputs
  devcontainer-build-smoke:
    on:
      pull_request / push:
        paths:
          - .github/workflows/devcontainer-ci.yml
          - .devcontainer/**
          - .pre-commit-config.yaml
          - mise.toml
          - **/mise.toml
          - package.json / package-lock.json / Cargo.toml / Cargo.lock
          - **/package.json / **/package-lock.json / **/bun.lock
          - **/pyproject.toml / **/uv.lock / **/Cargo.toml / **/Cargo.lock
      merge_group:
        branches: [main]
    - docker/setup-buildx-action (enables type=gha cache driver)
    - docker/login-action (ghcr.io, GITHUB_TOKEN)
    - devcontainers/ci build + smoke command
    - Run smoke command: gh/mise/bash versions
```

Push and pull-request filtering intentionally track devcontainer inputs and
dependency/setup manifests instead of every source-file change. `mise.toml`
coverage is dynamic via globbed trigger patterns plus a guide test that scans
the repository for current `mise.toml` files. GitHub Actions does not currently
support `paths` filters for `merge_group`, so merge queue coverage remains
branch-scoped.

## Rust CI

```yaml
jobs:
  ci:
    env:
      CARGO_TARGET_DIR: ${{ github.workspace }}/target/rust
    - cd ugoite-core && uv run ty check .
    - cd ugoite-core && cargo fmt --check
    - cd ugoite-core && cargo clippy -- -D warnings
    - cd ugoite-core && cargo test --no-run
    - cd ugoite-core && uv run maturin develop
    - cd ugoite-core && uv run pytest -W error
    - cd ugoite-core && cargo llvm-cov --summary-only --fail-under-lines 45
    - cd ugoite-cli && cargo fmt --check
    - cd ugoite-cli && cargo clippy --no-default-features -- -D warnings
    - cd ugoite-cli && cargo test --no-default-features
```

Local `mise` tasks for `ugoite-core` and `ugoite-cli` also share `target/rust`,
clean package-specific crate artifacts before rebuild/test, and expose
`mise run cleanup:rust-targets` to remove both the shared target root and the
legacy `~/.cache/ugoite/ugoite-core/target` path when artifacts grow unexpectedly.

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
- **Rust fmt/lint/test parity**: `ugoite-minimum`, `ugoite-core`, and `ugoite-cli` run Rust quality gates before commit
- **Docsite parity hooks**: Lint, format check, typecheck, and validation test for `docsite/`
- **Yamllint**: Validates YAML syntax/style on committed YAML files
- **Actionlint**: Validates `.github/workflows/*` syntax and workflow semantics
- **Root artifact hygiene**: Blocks root-level files with placeholder-only content
- **Ty**: Type checks Python projects (`backend/` and `ugoite-core/`)

Conventional Commit enforcement (local):

```bash
npm install
npm run prepare
```

This enables Husky `commit-msg` hook and runs `commitlint` before commit is accepted.

## Release Process

1. **Conventional Commits** are required locally (Husky + Commitlint) and in CI (`commitlint-ci`).
2. **Static checks and tests** must pass through existing CI workflows and `All Tests Status`.
3. **All Tests Status** must stay focused on code-quality workflows and exclude release/publish automation (`Release CI`, `Release Publish`) so auxiliary release failures do not turn branch health red.
4. **Release CI** runs on pushes to `main` and uses release-please to create/update a release PR with SemVer planning when `RELEASE_PLEASE_TOKEN` is configured.
5. **Release automation bootstrap** is seeded from `.github/.release-please-manifest.json`, `package.json`, and `.github/release-please-config.json`'s `bootstrap-sha`; the manifest/package versions must start at `0.0.1`, and `bootstrap-sha` bounds pre-release-please history so old merge titles do not decide current release planning.
6. **Release CI authentication** must use a dedicated `RELEASE_PLEASE_TOKEN`. If that secret is unavailable, the workflow must no-op cleanly instead of falling back to `GITHUB_TOKEN` and turning `main` red on repository-level PR permission errors.
7. **Human review** must confirm the planned release scope before publishing.
8. **Release Publish** is manual (`workflow_dispatch`) and requires explicit `APPROVED` confirmation.
9. **Stable/alpha/beta channels** are validated by channel-specific SemVer patterns at publish time.

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
# Rust
cd ugoite-minimum && cargo fmt --check && cargo clippy -- -D warnings && cargo test
cd ../ugoite-core && uv run ty check . && cargo fmt --check && cargo clippy -- -D warnings && cargo test --no-run && RUSTFLAGS='-C debuginfo=0' uv run maturin develop && uv run pytest -W error
cd ../ugoite-cli && cargo fmt --check && cargo clippy --no-default-features -- -D warnings && cargo test --no-default-features

# Python
cd .. && uvx ruff format --check .
uvx ruff check --select ALL --ignore-noqa .
cd backend && uv run ty check . && uv run pytest -W error

# Docs
cd .. && uv run --with pytest --with pyyaml --with bashlex pytest docs/tests -W error
cd docsite && bun run lint && bun run format:check && bun run typecheck && bun run test:validation

# Frontend
cd ../frontend && biome ci . && bun run test:run --coverage

# E2E (requires servers running)
cd .. && mise run e2e

# Conventional commits + release metadata
npm run commitlint:range
```

Or use pre-commit:
```bash
uvx pre-commit run --all-files
```
