# CI/CD Pipeline

## GitHub Actions Workflows

| Workflow | File | Triggers | Purpose |
|----------|------|----------|---------|
| Python CI | `.github/workflows/python-ci.yml` | Push on `main`, PR, merge queue | Lint, type check, pytest |
| Rust CI | `.github/workflows/rust-ci.yml` | Push, PR, merge queue | Minimum/core/CLI format, lint, test, and coverage |
| Frontend CI | `.github/workflows/frontend-ci.yml` | Push, PR, merge queue | Lint (biome), tests with mandatory 100% coverage |
| Docsite CI | `.github/workflows/docsite-ci.yml` | Push, PR, merge queue | Lint, format check, typecheck, validation build, and tests with mandatory 100% coverage |
| E2E Tests | `.github/workflows/e2e-ci.yml` | Push, PR, merge queue | Path-aware smoke/full E2E with merge-queue full coverage |
| Docker Build CI | `.github/workflows/docker-build-ci.yml` | Push on `main`, PR, merge queue | Build backend/frontend images and validate compose |
| Devcontainer CI | `.github/workflows/devcontainer-ci.yml` | Push on `main`, PR, merge queue | Build/smoke devcontainer with authenticated pulls and in-workflow change detection |
| SBOM CI | `.github/workflows/sbom-ci.yml` | Push, PR, merge queue | Generate CycloneDX SBOMs, sign/attest, and run vulnerability gate |
| ScanCode | `.github/workflows/scancode.yml` | Push on `main`, PR, merge queue, manual | Run license/compliance and vulnerability scanning |
| Shell CI | `.github/workflows/shell-ci.yml` | Push on `main`, PR, merge queue | Run shell formatting, lint, and syntax checks |
| YAML Workflow CI | `.github/workflows/yaml-workflow-ci.yml` | Push on `main`, PR, merge queue | Run repository artifact hygiene, yamllint, and actionlint |
| README Command Guard | `.github/workflows/readme-command-guard.yml` | PR, merge queue | Keep canonical root commands documented |
| Commitlint CI | `.github/workflows/commitlint-ci.yml` | PR, merge queue | Enforce Conventional Commits |
| CodeQL | `.github/workflows/codeql.yml` | Push on `main`, PR, merge queue, schedule, manual | Native code scanning for Actions, JavaScript/TypeScript, Python, and Rust |
| PR Template Validation | `.github/workflows/pr-require-close-issue.yml` | PR body events via `pull_request_target` | Enforce required PR sections and accepted close/closes issue links |
| Required Status Checks | `.github/required-status-checks.json` | Repository ruleset on `main` pull requests and merge queue | Versioned source of truth for direct workflow summary checks, exclusions, and native code-scanning handoff |
| Release CI | `.github/workflows/release-ci.yml` | Push on `main` | Create/update release PR with release-please (no auto publish) |
| Release Publish | `.github/workflows/release-publish.yml` | Manual (`workflow_dispatch`) | Human-approved stable/alpha/beta GitHub release publish with GHCR image push and CLI release assets |

GitHub branch protection and merge queue now rely on GitHub-native required status
checks declared in `.github/required-status-checks.json`. Each required workflow
emits a stable summary check with the workflow name, so the repository ruleset
can require direct workflow health instead of a polling rollup workflow.
Required workflows must not depend on top-level `paths` filters that would make
a check disappear. Path-aware workflows such as Devcontainer CI perform
in-workflow change detection and still emit their summary check when the
expensive job is skipped. Release automation (`Release CI`, `Release Publish`)
stays excluded from required status checks, and CodeQL remains enforced through
the repository's native code-scanning rule rather than the required-status-check
list.

Backend image builds in Docker Build CI, E2E CI, and SBOM CI pass `ugoite-core`,
`ugoite-minimum`, and `ugoite-cli` as Buildx contexts so Rust path dependencies
resolve inside the container build. Docker Build CI, E2E CI, SBOM CI, and
Release Publish share the reusable `.github/workflows/docker-images.yml`
image-definition contract so image build behavior cannot silently drift between
CI validation, local-image artifact export, and release publishing.

E2E CI selects a deterministic tier before running tests. `merge_group` and
pushes to `main` always run the full compose-backed suite. Pull requests only
drop to the smoke tier when every changed file stays inside docs/docsite
metadata paths (`docs/**`, `docsite/**`, `README.md`, `LICENSE`, `AGENTS.md`,
and `.github/ISSUE_TEMPLATE/**`); any application or workflow-input change
keeps the full suite.

## Native Required Status Checks

The required-check contract is versioned in `.github/required-status-checks.json`.
That JSON file is the source of truth for:

- which direct workflow summary checks the `main only pr` ruleset requires
- which workflows are explicitly excluded from required status checks
- which code-scanning tools stay enforced through GitHub-native code-scanning rules

Repository settings must stay aligned by applying ruleset updates from that JSON
contract with `gh api` whenever the required-check list changes.

Each required workflow exposes a summary check that uses the workflow name as
its check context and fails if any upstream job in that workflow fails or is
cancelled. The summary check always runs with `if: ${{ always() }}` so GitHub
never leaves a required context in the `Expected` state when a workflow takes a
path-aware no-op.

| Required Check | Workflow | Events | Notes |
|----------------|----------|--------|-------|
| Commitlint CI | `.github/workflows/commitlint-ci.yml` | PR, merge queue | Direct summary check for Conventional Commit enforcement |
| Devcontainer CI | `.github/workflows/devcontainer-ci.yml` | Push on `main`, PR, merge queue | Uses an in-workflow change detector and only runs the expensive smoke build when tracked inputs changed |
| Docker Build CI | `.github/workflows/docker-build-ci.yml` | Push on `main`, PR, merge queue | Summary check covers compose validation plus reusable image build |
| Docsite CI | `.github/workflows/docsite-ci.yml` | Push on `main`, PR, merge queue | Direct summary check for docsite quality gates |
| E2E Tests | `.github/workflows/e2e-ci.yml` | Push on `main`, PR, merge queue | Summary check covers image export, tier selection, and Playwright execution |
| Frontend CI | `.github/workflows/frontend-ci.yml` | Push on `main`, PR, merge queue | Direct summary check for Biome + Vitest coverage |
| Python CI | `.github/workflows/python-ci.yml` | Push on `main`, PR, merge queue | Direct summary check for Ruff, ty, backend pytest, and docs tests |
| README Command Guard | `.github/workflows/readme-command-guard.yml` | PR, merge queue | Direct summary check for canonical root commands |
| Rust CI | `.github/workflows/rust-ci.yml` | Push on `main`, PR, merge queue | Direct summary check for minimum/core/CLI Rust gates |
| SBOM CI | `.github/workflows/sbom-ci.yml` | Push on `main`, PR, merge queue | Summary check covers image export plus SBOM/signing/security gates |
| ScanCode | `.github/workflows/scancode.yml` | Push on `main`, PR, merge queue | Direct summary check for compliance scanning |
| Shell CI | `.github/workflows/shell-ci.yml` | Push on `main`, PR, merge queue | Direct summary check for shell quality gates |
| YAML Workflow CI | `.github/workflows/yaml-workflow-ci.yml` | Push on `main`, PR, merge queue | Direct summary check for root hygiene and workflow linting |

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

## YAML / Workflow and Repository Artifact Hygiene

```yaml
jobs:
  ci:
    - bash scripts/check-root-artifact-hygiene.sh
    - yamllint ...
    - actionlint
```

The root artifact hygiene gate fails tracked paths that still match repository
ignore rules, it blocks tracked files under generated dependency or build
directories such as `node_modules/` and `target/`, and it rejects tracked
files larger than `1 MiB` unless they are explicitly allowlisted in
`scripts/check-root-artifact-hygiene.sh`.

## Rust CI

```yaml
jobs:
  ci:
    - cd ugoite-minimum && cargo fmt --check
    - cd ugoite-minimum && cargo clippy -- -D warnings
    - cd ugoite-minimum && cargo test
    - python3 scripts/check_minimum_coverage.py
    - cd ugoite-core && uv run ty check .
    - cd ugoite-core && cargo fmt --check
    - cd ugoite-core && cargo clippy -- -D warnings
    - cd ugoite-core && cargo test --no-run
    - cd ugoite-core && uv run maturin develop
    - cd ugoite-core && uv run pytest -W error
    - cd ugoite-core && cargo llvm-cov --summary-only --fail-under-lines 45
    - cd ugoite-cli && cargo fmt --check
    - cd ugoite-cli && cargo clippy --no-default-features -- -D warnings
    - cd ugoite-cli && cargo llvm-cov --summary-only --fail-under-lines 100 --no-default-features
```

## Frontend CI

```yaml
jobs:
  ci:
    - cd frontend && biome ci .
    - cd frontend && bun install
    - cd frontend && node ./node_modules/vitest/vitest.mjs run --coverage --maxWorkers=1
```

The root `mise run test` contract must enforce the same frontend 100% coverage
gate by depending on `//frontend:test:coverage`, so local verification and CI
fail for the same coverage regressions.

## Docsite CI

```yaml
jobs:
  ci:
    - cd docsite && bun run lint
    - cd docsite && bun run format:check
    - cd docsite && bun run typecheck
    - cd docsite && bun run test:validation
    - cd docsite && node ./node_modules/vitest/vitest.mjs run --coverage --maxWorkers=1
```

The root `mise run test` contract must enforce the same docsite 100% coverage
gate by depending on `//docsite:test:coverage`, so local verification and CI
fail for the same coverage regressions.

## E2E CI

```yaml
jobs:
  build-images:
    - reusable docker-images.yml export of local backend/frontend image archives
  select-tier:
    - merge_group / push => full
    - pull_request with docs/docsite-only paths => smoke
    - all other pull_request changes => full
  e2e:
    - download and load pre-built backend/frontend images
    - Start backend (background)
    - Start frontend (background)
    - Wait for servers
    - bash e2e/scripts/run-e2e-compose.sh "${{ needs.select-tier.outputs.test_type }}"
    timeout: 30 minutes
```

The shared compose runner remains the CI path. Local `mise run e2e` prefers
that same compose runner when Docker is available, and otherwise falls back to a
production-style host runner that keeps the same Playwright JUnit/no-skips
validation contract. CI reuses pre-built images by setting
`E2E_BUILD_IMAGES=false`, while Docker-enabled local runs build the images from
the current workspace before starting the compose stack.

## Devcontainer CI

```yaml
jobs:
  detect-devcontainer-inputs:
    env:
      DEVCONTAINER_INPUT_PATTERNS: |
        .github/workflows/devcontainer-ci.yml
        .devcontainer/**
        .pre-commit-config.yaml
        mise.toml
        **/mise.toml
        package.json
        **/package.json
        package-lock.json
        **/package-lock.json
        Cargo.toml
        **/Cargo.toml
        Cargo.lock
        **/Cargo.lock
        **/bun.lock
        **/pyproject.toml
        **/uv.lock
    - git diff-based change detection for PR and push
    - always run the detector on merge queue entries
    - include docs/tests/*.py in tracked devcontainer inputs
  version-consistency:
    - pytest docs/tests/test_guides.py::test_docs_req_ops_001_mise_versions_match_ci_pins
    - pytest docs/tests/test_guides.py::test_docs_req_ops_012_devcontainer_change_detection_covers_inputs
  devcontainer-build-smoke:
    if: detect-devcontainer-inputs.outputs.should_run == 'true'
    - docker/setup-buildx-action (enables type=gha cache driver)
    - docker/login-action (ghcr.io, GITHUB_TOKEN)
    - devcontainers/ci build + smoke command
    - Run smoke command: gh/mise/bash versions
  required-check:
    if: always()
    - summary check still reports success when the smoke build is skipped
```

Devcontainer CI now uses an in-workflow change detector instead of top-level
`paths` filters so the required summary check is always emitted on pull
requests, merge queue entries, and pushes to `main`. The detector reads
`DEVCONTAINER_INPUT_PATTERNS`, which must keep covering current and future
`mise.toml` files plus docs guide tests that validate devcontainer setup
contracts. When no tracked input changed on a pull request or `push`, the smoke
build is skipped but the summary check still reports success with an explicit
selection reason. `merge_group` always runs the smoke build because GitHub does
not support `paths` filtering there and branch health cannot depend on a
disappearing required check.

## Rust CI

```yaml
jobs:
  ci:
    env:
      CARGO_TARGET_DIR: ${{ github.workspace }}/target/rust
    - cd ugoite-minimum && cargo fmt --check
    - cd ugoite-minimum && cargo clippy -- -D warnings
    - cd ugoite-minimum && cargo test
    - python3 scripts/check_minimum_coverage.py
    - cd ugoite-core && uv run ty check .
    - cd ugoite-core && cargo fmt --check
    - cd ugoite-core && cargo clippy -- -D warnings
    - cd ugoite-core && cargo test --no-run
    - cd ugoite-core && uv run maturin develop
    - cd ugoite-core && uv run pytest -W error
    - cd ugoite-core && cargo llvm-cov --summary-only --fail-under-lines 45
    - cd ugoite-cli && cargo fmt --check
    - cd ugoite-cli && cargo clippy --no-default-features -- -D warnings
    - cd ugoite-cli && cargo llvm-cov --summary-only --fail-under-lines 100 --no-default-features
```

The package-local `mise run //ugoite-minimum:test` task installs
`cargo-llvm-cov` when needed and runs `python3 ../scripts/check_minimum_coverage.py`,
which executes `cargo llvm-cov --test test_coverage --json` and normalizes
delimiter-only line-mapping noise before enforcing the same 100% ugoite-minimum line-coverage gate.
That keeps the root `mise run test` path aligned with Rust CI while still surfacing
substantive uncovered lines from the portable crate.

Local `mise` tasks for `ugoite-core` and `ugoite-cli` also share `target/rust`.
The default `ugoite-core` build path stays incremental, and root `mise run
test` runs `//ugoite-core:build` before `//backend:test:no-build` and
`//ugoite-core:test:no-build` so one editable extension build is reused across
that local test workflow. `mise run //ugoite-core:build:clean` provides a
package-local destructive rebuild when the editable extension is stale.
The default `mise run //ugoite-cli:test` path stays incremental (`cargo test`),
while `mise run //ugoite-cli:test:clean` provides a package-local destructive
rerun when CLI artifacts are stale. `mise run cleanup:rust-targets` removes
both the shared target root and the legacy `~/.cache/ugoite/ugoite-core/target`
path when artifacts grow unexpectedly. Rust CI and pre-commit still enforce the
100% CLI line-coverage gate through `cargo llvm-cov`.

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
- **Rust fmt/lint/test parity**: `ugoite-minimum`, `ugoite-core`, and `ugoite-cli` run Rust quality gates before commit, with both `ugoite-minimum` and `ugoite-cli` enforcing 100% line coverage via `cargo llvm-cov`
- **Docsite parity hooks**: Lint, format check, typecheck, validation build, and 100% Vitest coverage for `docsite/`
- **Yamllint**: Validates YAML syntax/style on committed YAML files
- **Actionlint**: Validates `.github/workflows/*` syntax and workflow semantics
- **Root artifact hygiene**: Blocks placeholder files, tracked+ignored paths, generated dependency trees, and oversized tracked artifacts
- **Ty**: Type checks Python projects (`backend/` and `ugoite-core/`)

Conventional Commit enforcement (local):

```bash
npm install
npm run prepare
```

This enables Husky `commit-msg` hook and runs `commitlint` before commit is accepted.

The root `mise.toml` also declares explicit `[monorepo].config_roots` for package-level task configs so top-level `mise run dev`, `mise run test`, and `mise run e2e` stay warning-free on current mise releases.

## Release Process

1. **Conventional Commits** are required locally (Husky + Commitlint) and in CI (`commitlint-ci`).
2. **Static checks and tests** must pass through existing CI workflows and the native required checks declared in `.github/required-status-checks.json`.
3. **GitHub-native required status checks** must map directly to workflow summary jobs, exclude release/publish automation (`Release CI`, `Release Publish`), and leave CodeQL on the repository code-scanning rule instead of a synthetic rollup workflow.
4. **Release CI** runs on pushes to `main` and uses release-please to create/update a release PR with SemVer planning when `RELEASE_PLEASE_TOKEN` is configured.
5. **Release automation bootstrap** is seeded from `.github/.release-please-manifest.json`, `packages/ugoite/package.json`, and `.github/release-please-config.json`'s `bootstrap-sha`; the manifest/package versions must start at `0.0.1`, the repository root `package.json` must stay private tooling for Husky/commitlint only, and `bootstrap-sha` bounds pre-release-please history so old merge titles do not decide current release planning.
6. **Release CI authentication** must use a dedicated `RELEASE_PLEASE_TOKEN`. If that secret is unavailable, the workflow must no-op cleanly instead of falling back to `GITHUB_TOKEN` and turning `main` red on repository-level PR permission errors.
7. **Human review** must confirm the planned release scope before publishing.
8. **Release Publish** is manual (`workflow_dispatch`) and requires explicit `APPROVED` confirmation.
9. **Stable/alpha/beta channels** are validated by channel-specific SemVer patterns at publish time.
10. **Release Publish** authenticates to GHCR with `GITHUB_TOKEN`, pushes `ghcr.io/ugoite/ugoite/backend` and `ghcr.io/ugoite/ugoite/frontend`, keeps tags aligned to the requested version (`<semver>` plus `latest`/`stable` for stable releases, or `<channel>` for alpha/beta), checks out the requested target before generating notes, creating the draft GitHub Release, exporting exact-version container image archives, and finalizing that release, delegates release image archive export to `.github/workflows/docker-images.yml` with `export_artifacts: true`, uploads `ugoite-backend-v<version>.docker.tar.gz`, `ugoite-frontend-v<version>.docker.tar.gz`, and `docker-compose.release.yaml` as GitHub Release assets alongside matching `.sha256` checksum files, commits the workspace `Cargo.lock` so clean checkouts can honor `cargo build --locked`, builds CLI archives for `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, and `aarch64-apple-darwin` through `.github/workflows/cli-release-binaries.yml`, uses `scripts/render-cli-release-installer.sh` to generate matching `ugoite-v<version>-<target>.install.sh` assets, uses the standard `macos-15` runner for the Intel macOS cross-build and `macos-14` for the Apple Silicon build, uploads those assets plus `.sha256` checksum files, and then finalizes the release.
11. **Container quick start** must stay documented in `README.md`, `docs/guide/container-quickstart.md`, and `docker-compose.release.yaml` so users can download, load, and run release images without rebuilding from source, and those docs must keep a dedicated `Environment Variables` section aligned with the shipped release-compose overrides.
12. **CLI installation** must stay documented in `README.md`, `docs/guide/cli.md`, and `scripts/install-ugoite-cli.sh` so users can install the released CLI and run `ugoite --help` without cloning the repository; the docs must also expose exact per-target one-liners for the `ugoite-v<version>-<target>.install.sh` release assets.
13. **Release quick-start smoke validation** may be exercised with `scripts/verify-release-cli-quickstart.sh`, which must keep both the generic installer path and the per-target `.install.sh` asset path aligned with the documented `space list` and `create-space` workflow for an exact release version.
14. **Public installer package** metadata lives in `packages/ugoite/package.json`, must stay non-private with `publishConfig.access=public`, must remain packable via `npm pack --dry-run`, must expose `ugoite-install` as the package-managed bootstrap to the canonical `scripts/install-ugoite-cli.sh` flow, and `README.md` plus `docs/guide/cli.md` must make the split between the public package and the private root tooling package explicit.

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
cd ../ugoite-cli && cargo fmt --check && cargo clippy --no-default-features -- -D warnings && cargo llvm-cov --summary-only --fail-under-lines 100 --no-default-features

# Python
cd .. && uvx ruff format --check .
uvx ruff check --select ALL --ignore-noqa .
cd backend && uv run ty check . && uv run pytest -W error

# Docs
cd .. && uv run --with pytest --with pyyaml --with bashlex pytest docs/tests -W error
cd docsite && bun run lint && bun run format:check && bun run typecheck && bun run test:validation && node ./node_modules/vitest/vitest.mjs run --coverage --maxWorkers=1

# Frontend
cd ../frontend && biome ci . && bun run test:run --coverage

# E2E (authoritative local parity path)
cd .. && mise run e2e

# Fast local iteration only (not CI parity)
cd .. && mise run e2e:dev

# Conventional commits + release metadata
npm run commitlint:range
```

Or use pre-commit:
```bash
uvx pre-commit run --all-files
```
