"""Guide validation tests.

REQ-OPS-001: Developer guides must be present with valid bash snippets.
REQ-OPS-002: Docker build CI workflow must be declared.
REQ-OPS-005: YAML/workflow lint gates must be enforced in pre-commit and CI.
REQ-OPS-006: Rust pre-commit checks must match CI test coverage expectations.
REQ-OPS-007: Docsite quality parity must be enforced in pre-commit and CI.
REQ-OPS-008: PR template validation rules must be enforced in CI.
REQ-OPS-009: Release automation bootstrap and PR permissions must be documented.
REQ-OPS-010: Frontend local-dev proxy readiness must be declared.
REQ-OPS-011: Rust target cache discipline must be declared.
REQ-OPS-012: Devcontainer trigger paths must cover setup inputs.
REQ-OPS-013: All Tests Status must exclude release automation from branch health.
REQ-OPS-016: Local sample-data seeding must be discoverable from root dev tasks.
REQ-OPS-017: Release publish must ship container quick-start assets and document them.
REQ-OPS-018: CLI release binaries and install path must stay documented and wired.
REQ-OPS-019: Mise monorepo config roots must be explicit and complete.
REQ-OPS-020: ugoite-minimum must keep WASM gates and boundary docs explicit.
REQ-OPS-021: Frontend 100% coverage must be explicit in CI and root mise test.
REQ-OPS-022: E2E CI path-aware tiering must stay explicit for PRs and merge queue.
REQ-OPS-023: Public installer package must stay separate from private tooling.
REQ-OPS-024: Docsite 100% coverage must be explicit in CI and root mise test.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import tarfile
import textwrap
from hashlib import sha256
from pathlib import Path, PurePosixPath

import bashlex
import tomllib
import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
GUIDE_DIR = REPO_ROOT / "docs" / "guide"
DOCKER_BUILD_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "docker-build-ci.yml"
DOCSITE_CI_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "docsite-ci.yml"
E2E_CI_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "e2e-ci.yml"
FRONTEND_CI_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "frontend-ci.yml"
PYTHON_CI_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "python-ci.yml"
YAML_WORKFLOW_CI_WORKFLOW_PATH = (
    REPO_ROOT / ".github" / "workflows" / "yaml-workflow-ci.yml"
)
RELEASE_CI_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "release-ci.yml"
RELEASE_PUBLISH_WORKFLOW_PATH = (
    REPO_ROOT / ".github" / "workflows" / "release-publish.yml"
)
CLI_RELEASE_WORKFLOW_PATH = (
    REPO_ROOT / ".github" / "workflows" / "cli-release-binaries.yml"
)
ROOT_GITIGNORE_PATH = REPO_ROOT / ".gitignore"
ROOT_CARGO_LOCK_PATH = REPO_ROOT / "Cargo.lock"
RELEASE_CONFIG_PATH = REPO_ROOT / ".github" / "release-please-config.json"
RUST_CI_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "rust-ci.yml"
SCANCODE_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "scancode.yml"
SBOM_CI_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "sbom-ci.yml"
DOCKER_IMAGES_REUSABLE_WORKFLOW_PATH = (
    REPO_ROOT / ".github" / "workflows" / "docker-images.yml"
)
DEVCONTAINER_CI_WORKFLOW_PATH = (
    REPO_ROOT / ".github" / "workflows" / "devcontainer-ci.yml"
)
ALL_TESTS_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "all-tests-ci.yml"
PR_TEMPLATE_WORKFLOW_PATH = (
    REPO_ROOT / ".github" / "workflows" / "pr-require-close-issue.yml"
)
PRE_COMMIT_CONFIG_PATH = REPO_ROOT / ".pre-commit-config.yaml"
PR_TEMPLATE_PATH = REPO_ROOT / ".github" / "pull_request_template.md"
README_PATH = REPO_ROOT / "README.md"
MISE_PATH = REPO_ROOT / "mise.toml"
BACKEND_MISE_PATH = REPO_ROOT / "backend" / "mise.toml"
UGOITE_MINIMUM_MISE_PATH = REPO_ROOT / "ugoite-minimum" / "mise.toml"
UGOITE_CORE_MISE_PATH = REPO_ROOT / "ugoite-core" / "mise.toml"
UGOITE_CLI_MISE_PATH = REPO_ROOT / "ugoite-cli" / "mise.toml"
FRONTEND_MISE_PATH = REPO_ROOT / "frontend" / "mise.toml"
DOCSITE_MISE_PATH = REPO_ROOT / "docsite" / "mise.toml"
CLI_GUIDE_PATH = GUIDE_DIR / "cli.md"
INSTALL_CLI_SCRIPT_PATH = REPO_ROOT / "scripts" / "install-ugoite-cli.sh"
RELEASE_INSTALLER_RENDERER_PATH = (
    REPO_ROOT / "scripts" / "render-cli-release-installer.sh"
)
RELEASE_CLI_QUICKSTART_SCRIPT_PATH = (
    REPO_ROOT / "scripts" / "verify-release-cli-quickstart.sh"
)
CONTAINER_QUICKSTART_GUIDE_PATH = GUIDE_DIR / "container-quickstart.md"
DEV_SEED_SCRIPT_PATH = REPO_ROOT / "scripts" / "dev-seed.sh"
ENV_MATRIX_PATH = GUIDE_DIR / "env-matrix.md"
LOCAL_DEV_AUTH_GUIDE_PATH = REPO_ROOT / "docs" / "guide" / "local-dev-auth-login.md"
WAIT_FOR_HTTP_PATH = REPO_ROOT / "scripts" / "wait-for-http.sh"
RUST_TARGET_CLEANUP_PATH = REPO_ROOT / "scripts" / "cleanup-rust-targets.sh"
RELEASE_MANIFEST_PATH = REPO_ROOT / ".github" / ".release-please-manifest.json"
ROOT_PACKAGE_JSON_PATH = REPO_ROOT / "package.json"
PUBLIC_PACKAGE_DIR = REPO_ROOT / "packages" / "ugoite"
PUBLIC_PACKAGE_JSON_PATH = PUBLIC_PACKAGE_DIR / "package.json"
PUBLIC_PACKAGE_README_PATH = PUBLIC_PACKAGE_DIR / "README.md"
PUBLIC_PACKAGE_LICENSE_PATH = PUBLIC_PACKAGE_DIR / "LICENSE"
PUBLIC_PACKAGE_INSTALLER_PATH = PUBLIC_PACKAGE_DIR / "bin" / "ugoite-install"
CI_CD_SPEC_PATH = REPO_ROOT / "docs" / "spec" / "testing" / "ci-cd.md"
RELEASE_COMPOSE_PATH = REPO_ROOT / "docker-compose.release.yaml"
COLUMN_COUNT_THRESHOLD = 2
DOCKER_IMAGE_WORKFLOW_PATHS = (
    DOCKER_BUILD_WORKFLOW_PATH,
    E2E_CI_WORKFLOW_PATH,
    SBOM_CI_WORKFLOW_PATH,
)
REQUIRED_BACKEND_BUILD_CONTEXTS = {
    "core=./ugoite-core",
    "minimum=./ugoite-minimum",
    "module=./ugoite-cli",
}
REQUIRED_FRONTEND_BUILD_CONTEXTS = {"shared=./shared"}
REQUIRED_PRE_COMMIT_HOOKS = {"root-artifact-hygiene", "yamllint", "actionlint"}
REQUIRED_YAML_WORKFLOW_CI_STEPS = {
    "Check root artifact hygiene",
    "Run yamllint",
    "Run actionlint",
}
REQUIRED_ARTIFACT_HYGIENE_SPEC_SNIPPETS = [
    "scripts/check-root-artifact-hygiene.sh",
    "tracked paths that still match repository",
    "ignore rules",
    "node_modules/",
    "target/",
    "1 MiB",
]
REQUIRED_SHARED_RUST_TARGET_DIR = "../target/rust"
REQUIRED_RUST_PRE_COMMIT_HOOKS = {
    "rustfmt",
    "cargo-clippy-minimum",
    "cargo-clippy",
    "cargo-clippy-cli",
    "cargo-llvm-cov-minimum",
    "cargo-llvm-cov-core",
    "cargo-llvm-cov-cli",
}
REQUIRED_RUST_CI_STEPS = {
    "Run tests (minimum)",
    "Enforce Rust coverage floor (minimum)",
    "Enforce Rust coverage floor (cli)",
}
REQUIRED_MINIMUM_COVERAGE_COMMAND = "scripts/check_minimum_coverage.py"
REQUIRED_MINIMUM_COVERAGE_BACKING_COMMAND = "cargo llvm-cov --test test_coverage --json"
REQUIRED_MINIMUM_COVERAGE_STEP_NAME = "Enforce Rust coverage floor (minimum)"
REQUIRED_MINIMUM_COVERAGE_DOC_FRAGMENTS = {
    "100% ugoite-minimum line-coverage gate",
    "mise run //ugoite-minimum:test",
    REQUIRED_MINIMUM_COVERAGE_COMMAND,
    REQUIRED_MINIMUM_COVERAGE_BACKING_COMMAND,
}
REQUIRED_RELEASE_CI_PERMISSIONS = {
    "contents": "write",
    "issues": "write",
    "pull-requests": "write",
}
REQUIRED_RELEASE_CI_TOKEN_FRAGMENTS = {
    "RELEASE_PLEASE_ENABLED",
    "env.RELEASE_PLEASE_ENABLED == 'true'",
    "env.RELEASE_PLEASE_ENABLED != 'true'",
    "SKIP_NO_RELEASE_PLEASE_TOKEN",
}
REQUIRED_RELEASE_CI_DOC_FRAGMENTS = {
    "RELEASE_PLEASE_TOKEN",
    "no-op cleanly",
    "bootstrap-sha",
    "0.0.1",
    "packages/ugoite/package.json",
    "root `package.json` must stay private tooling",
}
REQUIRED_CLI_RELEASE_WORKFLOW_FRAGMENTS = {
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "macos-15",
    "cargo build --locked --release --bin ugoite --target",
    "gh release upload",
    "ugoite-v${VERSION}-",
    ".install.sh",
    "render-cli-release-installer.sh",
    "permissions:",
    "contents: write",
}
FORBIDDEN_CLI_RELEASE_RUNNER_FRAGMENTS = {"macos-13", "macos-15-large"}
REQUIRED_RELEASE_PUBLISH_CLI_FRAGMENTS = {
    "create-draft-release",
    "publish-cli-binaries",
    "./.github/workflows/cli-release-binaries.yml",
    "version: ${{ inputs.version }}",
    "target: ${{ inputs.target }}",
    "--draft",
    "--draft=false",
}
REQUIRED_INSTALL_CLI_SCRIPT_FRAGMENTS = {
    "UGOITE_VERSION",
    "UGOITE_INSTALL_DIR",
    "UGOITE_DOWNLOAD_BASE_URL",
    "UGOITE_TARGET_OVERRIDE",
    "releases/latest",
    "uname -s",
    "uname -m",
    "install -m 0755",
    "sha256sum",
    "shasum -a 256",
}
REQUIRED_CLI_INSTALLER_ASSET_FRAGMENTS = {
    "ugoite-v0.1.0-x86_64-unknown-linux-gnu.install.sh",
    "ugoite-v0.1.0-aarch64-unknown-linux-gnu.install.sh",
    "ugoite-v0.1.0-x86_64-apple-darwin.install.sh",
    "ugoite-v0.1.0-aarch64-apple-darwin.install.sh",
}
REQUIRED_CLI_README_FRAGMENTS = {
    "install-ugoite-cli.sh",
    "ugoite --help",
    "UGOITE_VERSION=0.1.0",
    *REQUIRED_CLI_INSTALLER_ASSET_FRAGMENTS,
}
REQUIRED_CLI_GUIDE_FRAGMENTS = {
    "install-ugoite-cli.sh",
    "ugoite --help",
    "cargo build",
    "cargo run -q -p ugoite-cli -- --help",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    *REQUIRED_CLI_INSTALLER_ASSET_FRAGMENTS,
}
REQUIRED_CLI_CICD_FRAGMENTS = {
    ".github/workflows/cli-release-binaries.yml",
    "Cargo.lock",
    "macos-15",
    "scripts/install-ugoite-cli.sh",
    "scripts/render-cli-release-installer.sh",
    ".install.sh",
    "ugoite --help",
}
REQUIRED_RELEASE_INSTALLER_RENDERER_FRAGMENTS = {
    "UGOITE_TARGET_OVERRIDE",
    "install-ugoite-cli.sh",
    "raw.githubusercontent.com",
    "UGOITE_GITHUB_REPO",
}
REQUIRED_RELEASE_DRAFT_CHECKOUT_REF = (
    "${{ inputs.target != '' && inputs.target || github.sha }}"
)
REQUIRED_RELEASE_PUBLISH_PERMISSIONS = {
    "contents": "write",
    "packages": "write",
}
REQUIRED_RELEASE_PUBLISH_WORKFLOW_FRAGMENTS = {
    "./.github/workflows/docker-images.yml",
    "push: true",
    "export_artifacts: true",
    "artifact_name: release-docker-images",
    (
        "backend_local_tag: ghcr.io/${{ github.repository }}/backend:"
        "${{ inputs.version }}"
    ),
    (
        "frontend_local_tag: ghcr.io/${{ github.repository }}/frontend:"
        "${{ inputs.version }}"
    ),
    "actions/download-artifact@v4",
    "ugoite-backend-v${VERSION}.docker.tar.gz",
    "ugoite-frontend-v${VERSION}.docker.tar.gz",
    "docker-compose.release.yaml",
    "version: ${{ inputs.version }}",
    "channel: ${{ inputs.channel }}",
    "target: ${{ inputs.target }}",
}
REQUIRED_PUBLIC_PACKAGE_DOC_FRAGMENTS = {
    "packages/ugoite/package.json",
    "root `package.json` must stay private tooling",
    "npm pack --dry-run",
    "ugoite-install",
}
REQUIRED_PUBLIC_PACKAGE_README_FRAGMENTS = {
    "npm install -g ugoite",
    "ugoite-install",
    "scripts/install-ugoite-cli.sh",
    "UGOITE_VERSION",
}
REQUIRED_PUBLIC_PACKAGE_INSTALLER_FRAGMENTS = {
    "--print-script-url",
    "UGOITE_VERSION",
    "UGOITE_GITHUB_REPO",
    "scripts/install-ugoite-cli.sh",
    "raw.githubusercontent.com",
}
REQUIRED_DOCKER_IMAGES_REUSABLE_FRAGMENTS = {
    "workflow_call",
    "docker/login-action@v4",
    "registry: ghcr.io",
    "ghcr.io/${{ github.repository }}/backend",
    "ghcr.io/${{ github.repository }}/frontend",
    "export_artifacts:",
    "artifact_name:",
    "backend_local_tag:",
    "frontend_local_tag:",
    (
        'docker image save "$BACKEND_LOCAL_TAG" | gzip > '
        "exported-images/backend-image.tar.gz"
    ),
    (
        'docker image save "$FRONTEND_LOCAL_TAG" | gzip > '
        "exported-images/frontend-image.tar.gz"
    ),
    "actions/upload-artifact@v7",
    "$IMAGE:latest",
    "$IMAGE:stable",
    "$IMAGE:$CHANNEL",
}
REQUIRED_RELEASE_QUICKSTART_README_FRAGMENTS = {
    "docker-compose.release.yaml",
    "releases/download/v${UGOITE_VERSION}/docker-compose.release.yaml",
    "ugoite-backend-v${UGOITE_VERSION}.docker.tar.gz",
    "ugoite-frontend-v${UGOITE_VERSION}.docker.tar.gz",
    "docker load",
    "ghcr.io/ugoite/ugoite/backend",
    "ghcr.io/ugoite/ugoite/frontend",
    (
        'UGOITE_VERSION="$UGOITE_VERSION" docker compose -f '
        "docker-compose.release.yaml up -d"
    ),
    "http://localhost:3000/login",
    "Continue with Mock OAuth",
    "Environment Variables",
    "UGOITE_SPACES_DIR",
    "UGOITE_FRONTEND_PORT",
    "UGOITE_BACKEND_PORT",
    "UGOITE_DEV_USER_ID",
    "UGOITE_DEV_AUTH_PROXY_TOKEN",
}
REQUIRED_RELEASE_QUICKSTART_GUIDE_FRAGMENTS = {
    "releases/download/v${UGOITE_VERSION}/docker-compose.release.yaml",
    "ugoite-backend-v${UGOITE_VERSION}.docker.tar.gz",
    "ugoite-frontend-v${UGOITE_VERSION}.docker.tar.gz",
    "docker load",
    "ghcr.io/ugoite/ugoite/backend",
    "ghcr.io/ugoite/ugoite/frontend",
    (
        'UGOITE_VERSION="$UGOITE_VERSION" docker compose -f '
        "docker-compose.release.yaml up -d"
    ),
    "latest",
    "stable",
    "alpha",
    "beta",
    "http://localhost:3000/login",
    "Continue with Mock OAuth",
    "## Environment Variables",
    "UGOITE_SPACES_DIR",
    "UGOITE_FRONTEND_PORT",
    "UGOITE_BACKEND_PORT",
    "UGOITE_DEV_USER_ID",
    "UGOITE_DEV_AUTH_PROXY_TOKEN",
}
REQUIRED_RELEASE_COMPOSE_FRAGMENTS = {
    "ghcr.io/ugoite/ugoite/backend:${UGOITE_VERSION:?set UGOITE_VERSION}",
    "ghcr.io/ugoite/ugoite/frontend:${UGOITE_VERSION:?set UGOITE_VERSION}",
    "127.0.0.1:${UGOITE_FRONTEND_PORT:-3000}:3000",
    "127.0.0.1:${UGOITE_BACKEND_PORT:-8000}:8000",
    "${UGOITE_SPACES_DIR:-./spaces}:/data",
    "UGOITE_ROOT=/data",
    "BACKEND_URL=http://backend:8000",
    "UGOITE_DEV_AUTH_MODE=mock-oauth",
    "UGOITE_DEV_USER_ID=${UGOITE_DEV_USER_ID:-dev-local-user}",
    "UGOITE_DEV_SIGNING_KID=release-compose-local-v1",
    "UGOITE_DEV_SIGNING_SECRET=release-compose-local-secret",
    "UGOITE_AUTH_BEARER_SECRETS=release-compose-local-v1:release-compose-local-secret",
    "UGOITE_AUTH_BEARER_ACTIVE_KIDS=release-compose-local-v1",
    "UGOITE_DEV_AUTH_PROXY_TOKEN=${UGOITE_DEV_AUTH_PROXY_TOKEN:-release-compose-auth-proxy}",
}
REQUIRED_RELEASE_QUICKSTART_CICD_FRAGMENTS = {
    "ghcr.io/ugoite/ugoite/backend",
    "ghcr.io/ugoite/ugoite/frontend",
    "docker-compose.release.yaml",
    "ugoite-backend-v<version>.docker.tar.gz",
    "ugoite-frontend-v<version>.docker.tar.gz",
    "download, load, and run",
    "Environment Variables",
}
REQUIRED_DOCSITE_PRE_COMMIT_HOOKS = {
    "docsite-biome-ci",
    "docsite-format-check",
    "docsite-typecheck",
    "docsite-validation-test",
    "docsite-vitest-coverage",
}
REQUIRED_DOCSITE_CI_STEPS = {
    "Lint",
    "Format check",
    "Typecheck",
    "Validation test (build)",
    "Run docsite Vitest with 100% coverage gate",
}
REQUIRED_FRONTEND_DEV_FRAGMENTS = {
    "../scripts/wait-for-http.sh http://localhost:8000/health 30",
    "BACKEND_URL=http://localhost:8000",
    "bun run dev",
}
REQUIRED_WAIT_FOR_HTTP_FRAGMENTS = {
    "curl -fsS",
    "--connect-timeout",
    "--max-time",
    "Timed out waiting",
}
REQUIRED_LOCAL_DEV_AUTH_GUIDE_FRAGMENTS = {
    "/api/*",
    "waits for `http://localhost:8000/health`",
}
REQUIRED_LOCAL_DEV_AUTH_MODE_GUIDE_FRAGMENTS = {
    "UGOITE_DEV_AUTH_MODE",
    "manual-totp",
    "mock-oauth",
    "UGOITE_DEV_USER_ID",
    "UGOITE_DEV_AUTH_FORCE_LOGIN",
    "oathtool",
    "/login",
    "ugoite auth login",
    "signed bearer token",
    "0600",
}
REQUIRED_LOCAL_DEV_AUTH_MODE_README_FRAGMENTS = {
    "Local Dev Auth/Login",
    "canonical `mise run dev` workflow",
    "/login",
}
FORBIDDEN_LOCAL_DEV_AUTH_MODE_README_FRAGMENTS = {
    "UGOITE_DEV_AUTH_FORCE_LOGIN=true mise run dev",
    "UGOITE_DEV_AUTH_MODE=manual-totp",
    "UGOITE_DEV_AUTH_MODE=mock-oauth",
}
LOCAL_DEV_AUTH_GUIDE_EXTERNAL_URL = (
    "https://github.com/ugoite/ugoite/blob/main/docs/guide/local-dev-auth-login.md"
)
REQUIRED_LOCAL_DEV_AUTH_MODE_ENV_MATRIX_VARS = {
    "| UGOITE_DEV_AUTH_MODE |",
    "| UGOITE_DEV_USER_ID |",
    "| UGOITE_DEV_AUTH_FORCE_LOGIN |",
    "| UGOITE_DEV_2FA_SECRET |",
}
REQUIRED_LOCAL_DEV_AUTH_SCRIPT_FRAGMENTS = {
    'AUTH_MODE="${UGOITE_DEV_AUTH_MODE:-manual-totp}"',
    "UGOITE_DEV_USER_ID",
    "UGOITE_DEV_AUTH_FORCE_LOGIN",
    "UGOITE_DEV_SIGNING_SECRET",
    "UGOITE_DEV_SIGNING_KID",
    "path.chmod(0o600)",
    'announce_mode "manual-totp"',
    'announce_mode "mock-oauth"',
    "Local dev username:",
    "Current 2FA code:",
    "Unsupported UGOITE_DEV_AUTH_MODE",
}
REQUIRED_ALL_TESTS_EXCLUDED_WORKFLOWS = {"Release CI", "Release Publish"}
REQUIRED_ALL_TESTS_CURATED_WORKFLOWS = {
    "CodeQL",
    "Commitlint CI",
    "Devcontainer CI",
    "Docker Build CI",
    "Docsite CI",
    "E2E Tests",
    "Frontend CI",
    "Python CI",
    "README Command Guard",
    "Rust CI",
    "SBOM CI",
    "ScanCode",
    "Shell CI",
    "YAML Workflow CI",
}
REQUIRED_ALL_TESTS_DOC_FRAGMENTS = {
    "| All Tests Status | `.github/workflows/all-tests-ci.yml` |",
    "curated code-quality workflows",
    "deprecated wait-action runtimes",
    "exclude release/publish automation",
    "Release CI",
    "Release Publish",
}
REQUIRED_DEV_SEED_SCRIPT_FRAGMENTS = {
    "CARGO_TARGET_DIR",
    "UGOITE_SEED_SPACE_ID",
    "UGOITE_SEED_SCENARIO",
    "UGOITE_SEED_ENTRY_COUNT",
    "ugoite-cli",
    "sample-data",
    "Refusing to overwrite existing local sample space",
}
REQUIRED_DEV_SEED_README_FRAGMENTS = {
    "mise run seed",
    "UGOITE_SEED_SPACE_ID=ux-demo",
    "mise run seed:scenarios",
}
REQUIRED_MINIMUM_WASM_PRE_COMMIT_HOOKS = {
    "cargo-test-minimum",
    "cargo-wasm-minimum",
}
REQUIRED_MINIMUM_WASM_CI_STEPS = {
    "Run tests (minimum)",
    "Install wasm32 target (minimum)",
    "Build wasm32 (minimum)",
}
REQUIRED_MINIMUM_BOUNDARY_README_FRAGMENTS = {
    "wasm32-unknown-unknown",
    "compute_word_count",
    "OpenDAL",
    "ugoite-core",
    "ugoite-minimum",
    "//ugoite-minimum:build:wasm",
}
REQUIRED_MINIMUM_BOUNDARY_DOC_FRAGMENTS = {
    "`ugoite-minimum`",
    "`ugoite-core`",
    "compute_word_count",
    "OpenDAL",
}
EXPECTED_MONOREPO_CONFIG_ROOTS = {
    "backend",
    "docsite",
    "e2e",
    "frontend",
    "ugoite-cli",
    "ugoite-core",
    "ugoite-minimum",
}
REQUIRED_MONOREPO_DOC_FRAGMENTS = {
    "[monorepo].config_roots",
    "mise run dev",
    "mise run test",
    "mise run e2e",
}
REQUIRED_FRONTEND_COVERAGE_DOC_FRAGMENTS = {
    "100% coverage",
    "//frontend:test:coverage",
    "node ./node_modules/vitest/vitest.mjs run --coverage",
}
REQUIRED_FRONTEND_COVERAGE_COMMAND = (
    "node ./node_modules/vitest/vitest.mjs run --coverage"
)
REQUIRED_FRONTEND_COVERAGE_STEP_NAME = "Run Vitest with 100% coverage gate"
REQUIRED_DOCSITE_COVERAGE_DOC_FRAGMENTS = {
    "100% coverage",
    "//docsite:test:coverage",
    "node ./node_modules/vitest/vitest.mjs run --coverage --maxWorkers=1",
}
REQUIRED_DOCSITE_COVERAGE_COMMAND = (
    "node ./node_modules/vitest/vitest.mjs run --coverage --maxWorkers=1"
)
REQUIRED_DOCSITE_COVERAGE_STEP_NAME = "Run docsite Vitest with 100% coverage gate"
UV_SETUP_WORKFLOW_PATHS = (
    DEVCONTAINER_CI_WORKFLOW_PATH,
    PYTHON_CI_WORKFLOW_PATH,
    RUST_CI_WORKFLOW_PATH,
    YAML_WORKFLOW_CI_WORKFLOW_PATH,
)
REQUIRED_DEV_SEED_CLI_GUIDE_FRAGMENTS = {
    (
        "bash scripts/dev-seed.sh --space-id cli-demo --scenario lab-qa "
        "--entry-count 10 --seed 7"
    ),
    "UGOITE_SEED_SCENARIO=supply-chain",
    "CARGO_TARGET_DIR=target/rust cargo run -q -p ugoite-cli -- space sample-scenarios",
}
REQUIRED_DEVCONTAINER_TRIGGER_PATTERNS = {
    ".github/workflows/devcontainer-ci.yml",
    ".devcontainer/**",
    ".pre-commit-config.yaml",
    "mise.toml",
    "**/mise.toml",
    "package.json",
    "**/package.json",
    "package-lock.json",
    "**/package-lock.json",
    "Cargo.toml",
    "**/Cargo.toml",
    "Cargo.lock",
    "**/Cargo.lock",
    "**/bun.lock",
    "**/pyproject.toml",
    "**/uv.lock",
}

CODE_BLOCK_PATTERN = re.compile(
    r"```(?:bash|sh|shell)\s*\n(.*?)\n```",
    re.DOTALL,
)
ENV_NAME_PATTERN = re.compile(r"\b(?:UGOITE_[A-Z0-9_]+|E2E_[A-Z0-9_]+|BACKEND_URL)\b")


def _iter_bash_blocks(text: str) -> list[str]:
    return [block.strip() for block in CODE_BLOCK_PATTERN.findall(text)]


def _assert_no_shell_prompts(script: str, source: Path) -> None:
    for raw_line in script.splitlines():
        stripped = raw_line.lstrip()
        if stripped.startswith(("$ ", "> ")):
            message = (
                "Shell prompts are not allowed in bash blocks "
                f"({source.relative_to(REPO_ROOT)})"
            )
            raise AssertionError(message)


def _bash_syntax_check(script: str, source: Path) -> None:
    normalized = textwrap.dedent(script).strip()
    if not normalized:
        message = f"Empty bash block found in {source.relative_to(REPO_ROOT)}"
        raise AssertionError(message)

    _assert_no_shell_prompts(normalized, source)
    try:
        bashlex.parse(normalized)
    except bashlex.errors.ParsingError as exc:
        message = f"Bash syntax check failed for {source.relative_to(REPO_ROOT)}: {exc}"
        raise AssertionError(message) from exc


def test_docs_req_ops_001_guides_exist() -> None:
    """REQ-OPS-001: Required guide files must exist."""
    expected = {GUIDE_DIR / "docker-compose.md", GUIDE_DIR / "cli.md"}
    missing = [path for path in expected if not path.exists()]
    if missing:
        missing_list = ", ".join(str(path.relative_to(REPO_ROOT)) for path in missing)
        message = f"Missing guide files: {missing_list}"
        raise AssertionError(message)


def test_docs_req_ops_001_shell_blocks_parse() -> None:
    """REQ-OPS-001: Bash code blocks must be syntactically valid."""
    guide_files = sorted(GUIDE_DIR.glob("*.md"))
    if not guide_files:
        message = "No guide files found to validate"
        raise AssertionError(message)

    for guide_path in guide_files:
        content = guide_path.read_text(encoding="utf-8")
        for block in _iter_bash_blocks(content):
            _bash_syntax_check(block, guide_path)


def test_docs_req_ops_001_readme_core_commands_match_mise() -> None:
    """REQ-OPS-001: README onboarding commands must match root mise commands."""
    readme = README_PATH.read_text(encoding="utf-8")
    mise = MISE_PATH.read_text(encoding="utf-8")

    documented_tasks = sorted(set(re.findall(r"mise run ([a-zA-Z0-9:_-]+)", readme)))
    if not documented_tasks:
        message = "README must include at least one `mise run` command"
        raise AssertionError(message)

    for task in documented_tasks:
        escaped = re.escape(task)
        if not re.search(rf'^\[tasks\.(?:{escaped}|"{escaped}")\]', mise, re.MULTILINE):
            message = f"README command drift detected: missing mise task `{task}`"
            raise AssertionError(message)

    mise_data = tomllib.loads(mise)
    e2e_run = mise_data.get("tasks", {}).get("e2e", {}).get("run")
    if e2e_run != "bash e2e/scripts/run-e2e-parity.sh full":
        message = "Root `mise run e2e` must use the shared E2E parity wrapper"
        raise AssertionError(message)

    e2e_ci_workflow = E2E_CI_WORKFLOW_PATH.read_text(encoding="utf-8")
    if "bash e2e/scripts/run-e2e-compose.sh" not in e2e_ci_workflow:
        message = "E2E CI workflow must use the shared docker-compose E2E runner"
        raise AssertionError(message)


def test_docs_req_ops_001_env_matrix_matches_runtime_usage() -> None:
    """REQ-OPS-001: Environment matrix must track runtime variables used by tooling."""
    matrix_text = ENV_MATRIX_PATH.read_text(encoding="utf-8")
    documented = {
        line.split("|")[1].strip()
        for line in matrix_text.splitlines()
        if line.startswith("|")
        and len(line.split("|")) > COLUMN_COUNT_THRESHOLD
        and line.split("|")[1].strip() not in {"Variable", "---"}
    }

    source_paths = [
        REPO_ROOT / "docker-compose.yaml",
        REPO_ROOT / "e2e/scripts/run-e2e.sh",
        REPO_ROOT / "e2e/scripts/run-e2e-compose.sh",
        REPO_ROOT / ".github/workflows/e2e-ci.yml",
        REPO_ROOT / ".github/workflows/frontend-ci.yml",
        REPO_ROOT / "frontend/src/routes/api/[...path].ts",
    ]

    referenced: set[str] = set()
    for source_path in source_paths:
        text = source_path.read_text(encoding="utf-8")
        referenced.update(ENV_NAME_PATTERN.findall(text))

    missing = sorted(referenced - documented)
    if missing:
        raise AssertionError(
            "env-matrix.md is missing runtime env vars: " + ", ".join(missing),
        )


def test_docs_req_ops_001_mise_versions_match_ci_pins() -> None:
    """REQ-OPS-001: mise tool versions must match pinned versions in CI workflows."""
    mise_data = tomllib.loads(MISE_PATH.read_text(encoding="utf-8"))
    tools = mise_data.get("tools", {})
    if not isinstance(tools, dict):
        message = "mise.toml must define [tools]"
        raise TypeError(message)

    expected_tools = {
        "biome": _extract_single_workflow_pin_value(
            [FRONTEND_CI_WORKFLOW_PATH],
            uses_fragment="setup-biome",
            key="version",
            label="CI biome pins",
        ),
        "bun": _extract_single_workflow_pin_value(
            [DOCSITE_CI_WORKFLOW_PATH, FRONTEND_CI_WORKFLOW_PATH],
            uses_fragment="setup-bun",
            key="bun-version",
            label="CI bun-version pins",
        ),
        "python": _extract_single_workflow_pin_value(
            [SCANCODE_WORKFLOW_PATH],
            uses_fragment="scancode-action",
            key="python-version",
            label="CI python-version pins",
        ),
        "rust": _extract_single_workflow_pin_value(
            [RUST_CI_WORKFLOW_PATH],
            uses_fragment="rust-toolchain",
            key="toolchain",
            label="CI rust toolchain pins",
        ),
        "uv": _extract_single_workflow_pin_value(
            list(UV_SETUP_WORKFLOW_PATHS),
            uses_fragment="setup-uv",
            key="version",
            label="CI uv-version pins",
        ),
    }

    mismatches = sorted(
        f"{tool}={tools.get(tool)!r} (expected {expected})"
        for tool, expected in expected_tools.items()
        if str(tools.get(tool)) != expected
    )
    if mismatches:
        message = "mise.toml [tools] drift: " + "; ".join(mismatches)
        raise AssertionError(message)


def test_docs_req_ops_002_docker_build_ci_declared() -> None:
    """REQ-OPS-002: Docker build CI workflow must include backend and frontend."""
    missing_workflows = [
        str(workflow_path.relative_to(REPO_ROOT))
        for workflow_path in DOCKER_IMAGE_WORKFLOW_PATHS
        if not workflow_path.exists()
    ]
    if missing_workflows:
        message = "Missing workflow files: " + ", ".join(missing_workflows)
        raise AssertionError(message)

    missing_parts: list[str] = []
    for workflow_path in DOCKER_IMAGE_WORKFLOW_PATHS:
        workflow = _load_workflow(workflow_path)
        build_steps = _collect_build_steps(workflow)
        backend_step = _find_build_step(build_steps, "./backend")
        frontend_step = _find_build_step(build_steps, "./frontend")
        workflow_name = workflow_path.name
        _require_step(f"{workflow_name} backend", backend_step, missing_parts)
        _require_step(f"{workflow_name} frontend", frontend_step, missing_parts)
        _require_build_contexts(
            f"{workflow_name} backend",
            backend_step,
            REQUIRED_BACKEND_BUILD_CONTEXTS,
            missing_parts,
        )
        _require_build_contexts(
            f"{workflow_name} frontend",
            frontend_step,
            REQUIRED_FRONTEND_BUILD_CONTEXTS,
            missing_parts,
        )
    _raise_if_missing(missing_parts)


def test_docs_req_ops_005_yaml_workflow_lint_gates_declared() -> None:
    """REQ-OPS-005: YAML and workflow lint gates must exist in pre-commit and CI."""
    pre_commit = _load_pre_commit_config()
    configured_hooks = _collect_pre_commit_hook_ids(pre_commit)
    missing_hooks = sorted(REQUIRED_PRE_COMMIT_HOOKS.difference(configured_hooks))

    ci_steps = _collect_workflow_step_names(YAML_WORKFLOW_CI_WORKFLOW_PATH)
    missing_steps = sorted(REQUIRED_YAML_WORKFLOW_CI_STEPS.difference(ci_steps))
    python_ci_steps = _collect_workflow_step_names(PYTHON_CI_WORKFLOW_PATH)
    leaked_steps = sorted(REQUIRED_YAML_WORKFLOW_CI_STEPS.intersection(python_ci_steps))
    spec_detail = _require_file_contains(
        CI_CD_SPEC_PATH,
        REQUIRED_ARTIFACT_HYGIENE_SPEC_SNIPPETS,
        "ci-cd spec must document the root artifact hygiene guard",
    )

    if missing_hooks or missing_steps or leaked_steps or spec_detail:
        details: list[str] = []
        if missing_hooks:
            details.append("pre-commit missing hooks: " + ", ".join(missing_hooks))
        if missing_steps:
            details.append(
                "yaml-workflow-ci missing steps: " + ", ".join(missing_steps),
            )
        if leaked_steps:
            details.append("python-ci should not include: " + ", ".join(leaked_steps))
        if spec_detail:
            details.append(spec_detail)
        raise AssertionError("; ".join(details))


def test_docs_req_ops_006_rust_precommit_parity() -> None:
    """REQ-OPS-006: Rust pre-commit checks must keep minimum and CLI parity with CI."""
    missing_parts = _collect_req_ops_006_rust_precommit_parity_details()
    if missing_parts:
        raise AssertionError("; ".join(missing_parts))


def _collect_req_ops_006_rust_precommit_parity_details() -> list[str]:
    return [
        *_collect_req_ops_006_pre_commit_details(),
        *_collect_req_ops_006_ci_details(),
    ]


def _collect_req_ops_006_pre_commit_details() -> list[str]:
    pre_commit = _load_pre_commit_config()
    configured_hooks = _collect_pre_commit_hook_ids(pre_commit)
    missing_hooks = sorted(REQUIRED_RUST_PRE_COMMIT_HOOKS.difference(configured_hooks))

    hook_entries = _collect_pre_commit_hooks(pre_commit)
    clippy_cli_entry = ""
    cargo_clippy_cli = hook_entries.get("cargo-clippy-cli")
    if isinstance(cargo_clippy_cli, dict):
        clippy_cli_entry = str(cargo_clippy_cli.get("entry", ""))

    minimum_cov_entry = ""
    cargo_cov_minimum = hook_entries.get("cargo-llvm-cov-minimum")
    if isinstance(cargo_cov_minimum, dict):
        minimum_cov_entry = str(cargo_cov_minimum.get("entry", ""))
    cli_cov_entry = ""
    cargo_llvm_cov_cli = hook_entries.get("cargo-llvm-cov-cli")
    if isinstance(cargo_llvm_cov_cli, dict):
        cli_cov_entry = str(cargo_llvm_cov_cli.get("entry", ""))

    missing_parts: list[str] = []
    if missing_hooks:
        missing_parts.append("pre-commit missing hooks: " + ", ".join(missing_hooks))
    if "--no-default-features" not in clippy_cli_entry:
        missing_parts.append("cargo-clippy-cli must pass --no-default-features")
    if REQUIRED_MINIMUM_COVERAGE_COMMAND not in minimum_cov_entry:
        missing_parts.append("cargo-llvm-cov-minimum must run the wrapper")
    if "--fail-under-lines 100" not in cli_cov_entry:
        missing_parts.append("cargo-llvm-cov-cli must enforce 100% line coverage")
    if "--no-default-features" not in cli_cov_entry:
        missing_parts.append("cargo-llvm-cov-cli must pass --no-default-features")

    return missing_parts


def _collect_req_ops_006_ci_details() -> list[str]:
    missing_parts: list[str] = []
    rust_ci_steps = _collect_workflow_step_names(RUST_CI_WORKFLOW_PATH)
    missing_ci_steps = sorted(REQUIRED_RUST_CI_STEPS.difference(rust_ci_steps))
    root_mise = tomllib.loads(MISE_PATH.read_text(encoding="utf-8"))
    root_runs = _get_task_run_commands(root_mise, "test")
    minimum_mise = tomllib.loads(UGOITE_MINIMUM_MISE_PATH.read_text(encoding="utf-8"))
    minimum_task = _load_mise_task_mapping(
        minimum_mise,
        task_name="test",
        path_label="ugoite-minimum/mise.toml",
    )
    minimum_run = _load_task_run(
        minimum_task,
        task_label="ugoite-minimum/mise.toml [tasks.test]",
    )
    minimum_step_run = _find_workflow_step_run(
        RUST_CI_WORKFLOW_PATH,
        job_name="ci",
        step_name=REQUIRED_MINIMUM_COVERAGE_STEP_NAME,
    )
    guide_text = CI_CD_SPEC_PATH.read_text(encoding="utf-8")
    missing_doc_fragments = sorted(
        fragment
        for fragment in REQUIRED_MINIMUM_COVERAGE_DOC_FRAGMENTS
        if fragment not in guide_text
    )

    workflow_text = RUST_CI_WORKFLOW_PATH.read_text(encoding="utf-8")
    cli_coverage_command = (
        "cargo llvm-cov --summary-only --fail-under-lines 100 --no-default-features"
    )
    if missing_ci_steps:
        missing_parts.append("rust-ci missing steps: " + ", ".join(missing_ci_steps))
    if "mise run //ugoite-minimum:test" not in root_runs:
        missing_parts.append("root mise.toml tasks.test must run //ugoite-minimum:test")
    if REQUIRED_MINIMUM_COVERAGE_COMMAND not in minimum_run:
        missing_parts.append(
            "ugoite-minimum/mise.toml [tasks.test] must run the wrapper",
        )
    if minimum_step_run is None:
        missing_parts.append("rust-ci.yml must define the minimum coverage gate step")
    elif REQUIRED_MINIMUM_COVERAGE_COMMAND not in minimum_step_run:
        missing_parts.append("rust-ci minimum coverage gate must run the wrapper")
    if "components: rustfmt, clippy, llvm-tools-preview" not in workflow_text:
        missing_parts.append("rust-ci must install llvm-tools-preview")
    if cli_coverage_command not in workflow_text:
        missing_parts.append("rust-ci must enforce 100% ugoite-cli coverage")

    spec_detail = _require_file_contains(
        CI_CD_SPEC_PATH,
        [
            cli_coverage_command,
            "mise run //ugoite-cli:test",
            "100% CLI line-coverage gate",
        ],
        "ci-cd spec must document the ugoite-cli 100% coverage gate",
    )
    if spec_detail:
        missing_parts.append(spec_detail)

    if missing_doc_fragments:
        missing_parts.append(
            "ci-cd guide missing minimum coverage fragments: "
            + ", ".join(missing_doc_fragments),
        )

    return missing_parts


def test_docs_req_ops_007_docsite_quality_parity_declared() -> None:
    """REQ-OPS-007: Docsite lint/format/test parity must be wired in CI/pre-commit."""
    pre_commit = _load_pre_commit_config()
    configured_hooks = _collect_pre_commit_hook_ids(pre_commit)
    missing_hooks = sorted(
        REQUIRED_DOCSITE_PRE_COMMIT_HOOKS.difference(configured_hooks),
    )

    ci_step_names = _collect_workflow_step_names(DOCSITE_CI_WORKFLOW_PATH)
    missing_steps = sorted(REQUIRED_DOCSITE_CI_STEPS.difference(ci_step_names))

    if missing_hooks or missing_steps:
        details: list[str] = []
        if missing_hooks:
            details.append("pre-commit missing hooks: " + ", ".join(missing_hooks))
        if missing_steps:
            details.append("docsite-ci missing steps: " + ", ".join(missing_steps))
        raise AssertionError("; ".join(details))


def test_docs_req_ops_008_pr_template_validation_rules_declared() -> None:
    """REQ-OPS-008: PR workflow must enforce sections and close/closes links."""
    workflow_text = PR_TEMPLATE_WORKFLOW_PATH.read_text(encoding="utf-8")
    template_text = PR_TEMPLATE_PATH.read_text(encoding="utf-8")

    required_template_fragments = {
        "## Summary",
        "## Related Issue (required)",
        "## Testing",
    }
    missing_template_fragments = sorted(
        fragment
        for fragment in required_template_fragments
        if fragment not in template_text
    )

    required_workflow_fragments = {
        "## Summary",
        "## Related Issue (required)",
        "## Testing",
        "close:",
        "closes",
    }
    missing_workflow_fragments = sorted(
        fragment
        for fragment in required_workflow_fragments
        if fragment not in workflow_text
    )

    if missing_template_fragments or missing_workflow_fragments:
        details: list[str] = []
        if missing_template_fragments:
            details.append(
                "pull_request_template missing fragments: "
                + ", ".join(missing_template_fragments),
            )
        if missing_workflow_fragments:
            details.append(
                "pr-require-close-issue workflow missing fragments: "
                + ", ".join(missing_workflow_fragments),
            )
        raise AssertionError("; ".join(details))


def test_docs_req_ops_009_release_ci_bootstrap_and_permissions_declared() -> None:
    """REQ-OPS-009: Release CI bootstrap metadata and permissions stay aligned."""
    details = _collect_release_ci_requirement_details()
    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_ops_011_rust_target_cache_discipline_declared() -> None:
    """REQ-OPS-011: Rust target caches must stay bounded and cleanable."""
    root_mise = tomllib.loads(MISE_PATH.read_text(encoding="utf-8"))
    backend_mise = tomllib.loads(BACKEND_MISE_PATH.read_text(encoding="utf-8"))
    core_mise = tomllib.loads(UGOITE_CORE_MISE_PATH.read_text(encoding="utf-8"))
    cli_mise = tomllib.loads(UGOITE_CLI_MISE_PATH.read_text(encoding="utf-8"))

    details = [
        detail
        for detail in [
            _require_shared_target_dir(core_mise, "ugoite-core/mise.toml"),
            _require_shared_target_dir(cli_mise, "ugoite-cli/mise.toml"),
            _require_task_contains(
                core_mise,
                "build",
                "uv run maturin develop",
                "ugoite-core build task must build the editable Rust extension",
            ),
            _require_task_excludes(
                core_mise,
                "build",
                "cargo clean -p ugoite-core",
                "ugoite-core build task must stay incremental by default",
            ),
            _require_task_contains(
                core_mise,
                "build:clean",
                "cargo clean -p ugoite-core",
                "ugoite-core build:clean task must clean package-local Rust artifacts",
            ),
            _require_task_contains(
                core_mise,
                "build:clean",
                "uv run maturin develop",
                "ugoite-core build:clean task must rebuild the editable Rust extension",
            ),
            _require_exact_task_run(
                core_mise,
                "test:no-build",
                [
                    "cargo test -j 1",
                    (
                        "if [ -d tests ]; then uv run --with pytest --with "
                        "pytest-asyncio python -m pytest; fi"
                    ),
                ],
                (
                    "ugoite-core test:no-build task must run crate and Python "
                    "binding tests without rebuilding"
                ),
            ),
            _require_exact_task_depends(
                core_mise,
                "test",
                ["build"],
                "ugoite-core test task must depend on build",
            ),
            _require_exact_task_run(
                backend_mise,
                "test:no-build",
                ["uv run pytest"],
                "backend test:no-build task must run backend pytest directly",
            ),
            _require_exact_task_depends(
                backend_mise,
                "test",
                ["//ugoite-core:build"],
                "backend test task must depend on ugoite-core:build",
            ),
            _require_task_contains(
                cli_mise,
                "install",
                "cargo clean -p ugoite-cli",
                "ugoite-cli install task must clean package-local Rust artifacts",
            ),
            _require_task_contains(
                cli_mise,
                "test",
                "cargo test",
                "ugoite-cli test task must run cargo test",
            ),
            _require_task_excludes(
                cli_mise,
                "test",
                "cargo clean -p ugoite-cli",
                "ugoite-cli test task must stay incremental by default",
            ),
            _require_task_contains(
                cli_mise,
                "test:clean",
                "cargo clean -p ugoite-cli",
                "ugoite-cli test:clean task must clean package-local Rust artifacts",
            ),
            _require_task_contains(
                cli_mise,
                "test:clean",
                "cargo test",
                "ugoite-cli test:clean task must rerun cargo test after cleaning",
            ),
            _require_exact_task_run(
                root_mise,
                "cleanup:rust-targets",
                ["bash scripts/cleanup-rust-targets.sh"],
                "root mise must expose cleanup:rust-targets",
            ),
            _require_exact_task_run(
                root_mise,
                "test",
                [
                    "mise run //ugoite-core:build",
                    "mise run //backend:test:no-build",
                    "mise run //frontend:test:coverage",
                    "mise run //docsite:test:coverage",
                    "mise run //ugoite-cli:test",
                    "mise run //ugoite-core:test:no-build",
                    "mise run //ugoite-minimum:test",
                    "mise run test:docs",
                ],
                (
                    "root mise test must build ugoite-core once before "
                    "reusing no-build backend/core test tasks"
                ),
            ),
            _require_file_contains(
                RUST_TARGET_CLEANUP_PATH,
                ["target/rust", ".cache/ugoite/ugoite-core/target"],
                (
                    "cleanup-rust-targets.sh must remove shared and legacy "
                    "Rust target caches"
                ),
            ),
            _require_file_contains(
                RUST_CI_WORKFLOW_PATH,
                ["CARGO_TARGET_DIR: ${{ github.workspace }}/target/rust"],
                "rust-ci.yml must set CARGO_TARGET_DIR to the shared workspace root",
            ),
            _require_file_contains(
                README_PATH,
                [
                    "mise run cleanup:rust-targets",
                    "mise run //ugoite-core:build:clean",
                    "mise run //ugoite-cli:test:clean",
                ],
                (
                    "README must document cleanup:rust-targets and package-local "
                    "clean rebuild/test commands"
                ),
            ),
        ]
        if detail is not None
    ]
    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_ops_010_frontend_dev_proxy_readiness_declared() -> None:
    """REQ-OPS-010: Frontend local dev waits for backend readiness before proxying."""
    details = _collect_frontend_dev_proxy_readiness_details()
    if details:
        raise AssertionError("; ".join(details))


def _collect_frontend_dev_proxy_readiness_details() -> list[str]:
    if not FRONTEND_MISE_PATH.exists():
        message = f"frontend/mise.toml is missing; expected at {FRONTEND_MISE_PATH}"
        raise AssertionError(message)

    frontend_mise = tomllib.loads(FRONTEND_MISE_PATH.read_text(encoding="utf-8"))
    tasks = frontend_mise.get("tasks", {})
    if not isinstance(tasks, dict):
        message = "frontend/mise.toml must define [tasks]"
        raise TypeError(message)

    dev_task = tasks.get("dev")
    if not isinstance(dev_task, dict):
        message = "frontend/mise.toml must define [tasks.dev]"
        raise TypeError(message)

    run_command = dev_task.get("run")
    if not isinstance(run_command, str):
        message = "frontend [tasks.dev].run must be a string command"
        raise TypeError(message)

    wait_script = _read_required_text(
        WAIT_FOR_HTTP_PATH,
        "wait-for-http.sh is missing at {path}; required by REQ-OPS-010 for "
        "frontend dev proxy readiness.",
    )
    guide_text = _read_required_text(
        LOCAL_DEV_AUTH_GUIDE_PATH,
        "local dev auth guide is missing at {path}; required by REQ-OPS-010 for "
        "documenting frontend proxy readiness.",
    )

    missing_fragments = sorted(
        fragment
        for fragment in REQUIRED_FRONTEND_DEV_FRAGMENTS
        if fragment not in run_command
    )
    missing_wait_fragments = sorted(
        fragment
        for fragment in REQUIRED_WAIT_FOR_HTTP_FRAGMENTS
        if fragment not in wait_script
    )
    missing_guide_fragments = sorted(
        fragment
        for fragment in REQUIRED_LOCAL_DEV_AUTH_GUIDE_FRAGMENTS
        if fragment not in guide_text
    )

    details: list[str] = []
    if missing_fragments:
        details.append(
            "frontend dev command missing fragments: " + ", ".join(missing_fragments),
        )
    if missing_wait_fragments:
        details.append(
            "wait-for-http.sh missing readiness fragments: "
            + ", ".join(missing_wait_fragments),
        )
    if missing_guide_fragments:
        details.append(
            "local-dev-auth-login.md missing readiness fragments: "
            + ", ".join(missing_guide_fragments),
        )
    return details


def test_docs_req_ops_012_devcontainer_trigger_paths_cover_inputs() -> None:
    """REQ-OPS-012: Devcontainer triggers must cover setup inputs and mise.toml."""
    workflow = _load_yaml_base_mapping(DEVCONTAINER_CI_WORKFLOW_PATH)
    pull_request_paths = _collect_trigger_paths(workflow, "pull_request")
    push_paths = _collect_trigger_paths(workflow, "push")
    discovered_mise_paths = _discover_repo_paths("mise.toml")

    details: list[str] = []
    if not pull_request_paths:
        details.append("devcontainer-ci pull_request trigger must define paths")
    if not push_paths:
        details.append("devcontainer-ci push trigger must define paths")
    if pull_request_paths != push_paths:
        details.append("devcontainer-ci push and pull_request paths must match")

    missing_patterns = sorted(
        REQUIRED_DEVCONTAINER_TRIGGER_PATTERNS.difference(pull_request_paths),
    )
    if missing_patterns:
        details.append(
            "devcontainer-ci trigger paths missing patterns: "
            + ", ".join(missing_patterns),
        )

    uncovered_mise_paths = sorted(
        path
        for path in discovered_mise_paths
        if not _matches_any_workflow_pattern(path, pull_request_paths)
    )
    if uncovered_mise_paths:
        details.append(
            "devcontainer-ci trigger paths must cover all mise.toml files: "
            + ", ".join(uncovered_mise_paths),
        )

    step_names = _collect_workflow_step_names(DEVCONTAINER_CI_WORKFLOW_PATH)
    if "Check devcontainer trigger coverage (REQ-OPS-012)" not in step_names:
        details.append("devcontainer-ci must validate REQ-OPS-012 in CI")

    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_ops_015_local_dev_auth_docs_cover_manual_modes() -> None:
    """REQ-OPS-015: Local dev auth docs stay canonical and cover manual modes."""
    guide_text = LOCAL_DEV_AUTH_GUIDE_PATH.read_text(encoding="utf-8")
    readme_text = README_PATH.read_text(encoding="utf-8")
    env_matrix_text = ENV_MATRIX_PATH.read_text(encoding="utf-8")
    api_storage_text = (
        REPO_ROOT / "docsite" / "src" / "pages" / "app" / "api-storage" / "index.astro"
    ).read_text(encoding="utf-8")
    frontend_docsite_text = (
        REPO_ROOT / "docsite" / "src" / "pages" / "app" / "frontend" / "index.astro"
    ).read_text(encoding="utf-8")
    spaces_index_text = (
        REPO_ROOT / "frontend" / "src" / "routes" / "spaces" / "index.tsx"
    ).read_text(encoding="utf-8")
    space_settings_text = (
        REPO_ROOT
        / "frontend"
        / "src"
        / "routes"
        / "spaces"
        / "[space_id]"
        / "settings.tsx"
    ).read_text(encoding="utf-8")

    details: list[str] = []

    missing_guide = sorted(
        fragment
        for fragment in REQUIRED_LOCAL_DEV_AUTH_MODE_GUIDE_FRAGMENTS
        if fragment not in guide_text
    )
    if missing_guide:
        details.append(
            "local-dev-auth-login.md missing manual auth fragments: "
            + ", ".join(missing_guide),
        )

    missing_readme = sorted(
        fragment
        for fragment in REQUIRED_LOCAL_DEV_AUTH_MODE_README_FRAGMENTS
        if fragment not in readme_text
    )
    if missing_readme:
        details.append(
            "README missing manual auth fragments: " + ", ".join(missing_readme),
        )

    duplicated_readme = sorted(
        fragment
        for fragment in FORBIDDEN_LOCAL_DEV_AUTH_MODE_README_FRAGMENTS
        if fragment in readme_text
    )
    if duplicated_readme:
        details.append(
            "README must point to the canonical guide instead of duplicating auth "
            "commands: " + ", ".join(duplicated_readme),
        )

    missing_env_matrix = sorted(
        fragment
        for fragment in REQUIRED_LOCAL_DEV_AUTH_MODE_ENV_MATRIX_VARS
        if fragment not in env_matrix_text
    )
    if missing_env_matrix:
        details.append(
            "env-matrix.md missing manual auth vars: " + ", ".join(missing_env_matrix),
        )

    canonical_pointer_sources = {
        "docsite api-storage page": api_storage_text,
        "docsite frontend page": frontend_docsite_text,
        "frontend spaces route": spaces_index_text,
        "frontend space settings route": space_settings_text,
    }
    missing_canonical_pointers = sorted(
        name
        for name, text in canonical_pointer_sources.items()
        if "Local Dev Auth/Login" not in text
        or LOCAL_DEV_AUTH_GUIDE_EXTERNAL_URL not in text
    )
    if missing_canonical_pointers:
        details.append(
            "Local dev auth pointers missing canonical guide links in: "
            + ", ".join(missing_canonical_pointers),
        )

    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_ops_015_dev_auth_script_declares_manual_modes() -> None:
    """REQ-OPS-015: dev-auth-env.sh must declare the supported auth modes."""
    script_text = (REPO_ROOT / "scripts" / "dev-auth-env.sh").read_text(
        encoding="utf-8",
    )
    missing = sorted(
        fragment
        for fragment in REQUIRED_LOCAL_DEV_AUTH_SCRIPT_FRAGMENTS
        if fragment not in script_text
    )
    if missing:
        message = "dev-auth-env.sh missing manual auth mode fragments: " + ", ".join(
            missing,
        )
        raise AssertionError(message)


def test_docs_req_ops_015_mock_oauth_startup_avoids_terminal_prompts(
    tmp_path: Path,
) -> None:
    """REQ-OPS-015: mock-oauth startup stays explicit without terminal prompts."""
    auth_file = tmp_path / "dev-auth.json"
    script_path = REPO_ROOT / "scripts" / "dev-auth-env.sh"
    env = os.environ.copy()
    env.update(
        {
            "HOME": str(tmp_path),
            "UGOITE_DEV_AUTH_MODE": "mock-oauth",
            "UGOITE_DEV_AUTH_FILE": str(auth_file),
            "UGOITE_DEV_SIGNING_KID": "mock-oauth-test-kid",
            "UGOITE_DEV_SIGNING_SECRET": "mock-oauth-test-secret",
        },
    )
    env.pop("UGOITE_DEV_USER_ID", None)
    env.pop("UGOITE_DEV_TOTP_CODE", None)

    result = subprocess.run(
        ["/bin/bash", str(script_path)],
        cwd=REPO_ROOT,
        env=env,
        input="",
        text=True,
        capture_output=True,
        timeout=5,
        check=False,
    )

    if result.returncode != 0:
        message = (
            "mock-oauth startup should not block on terminal input; "
            f"stdout={result.stdout!r} stderr={result.stderr!r}"
        )
        raise AssertionError(message)

    if "Local dev username:" in result.stdout or "Local dev username:" in result.stderr:
        message = "mock-oauth startup must not prompt for a local username"
        raise AssertionError(message)

    if "Current 2FA code:" in result.stdout or "Current 2FA code:" in result.stderr:
        message = "mock-oauth startup must not prompt for a TOTP code"
        raise AssertionError(message)

    exports = result.stdout
    if "export UGOITE_DEV_AUTH_MODE=mock-oauth" not in exports:
        message = "mock-oauth startup must export the selected auth mode"
        raise AssertionError(message)
    if "export UGOITE_DEV_USER_ID=dev-local-user" not in exports:
        message = "mock-oauth startup must default UGOITE_DEV_USER_ID"
        raise AssertionError(message)

    auth_payload = json.loads(auth_file.read_text(encoding="utf-8"))
    if auth_payload["mode"] != "mock-oauth":
        message = "mock-oauth startup must persist the selected mode"
        raise AssertionError(message)
    if auth_payload["user_id"] != "dev-local-user":
        message = "mock-oauth startup must persist the default dev user"
        raise AssertionError(message)


def test_docs_req_ops_013_all_tests_status_excludes_release_automation() -> None:
    """REQ-OPS-013: All Tests Status must exclude release automation workflows."""
    details = _collect_all_tests_status_details()
    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_ops_016_dev_seed_workflow_is_declared() -> None:
    """REQ-OPS-016: Root dev tasks must expose local sample-data seeding."""
    root_mise = tomllib.loads(MISE_PATH.read_text(encoding="utf-8"))
    details = [
        detail
        for detail in [
            _require_exact_task_run(
                root_mise,
                "seed",
                ["bash scripts/dev-seed.sh"],
                "root mise must expose seed",
            ),
            _require_exact_task_run(
                root_mise,
                "seed:scenarios",
                [
                    "CARGO_TARGET_DIR=target/rust cargo run -q -p ugoite-cli -- "
                    "space sample-scenarios",
                ],
                "root mise must expose seed:scenarios",
            ),
            _require_file_contains(
                DEV_SEED_SCRIPT_PATH,
                sorted(REQUIRED_DEV_SEED_SCRIPT_FRAGMENTS),
                "scripts/dev-seed.sh must wrap CLI sample-data seeding safely",
            ),
            _require_file_contains(
                README_PATH,
                sorted(REQUIRED_DEV_SEED_README_FRAGMENTS),
                "README must document the local seed workflow",
            ),
            _require_file_contains(
                CLI_GUIDE_PATH,
                sorted(REQUIRED_DEV_SEED_CLI_GUIDE_FRAGMENTS),
                (
                    "docs/guide/cli.md must document seed task overrides "
                    "and direct CLI usage"
                ),
            ),
        ]
        if detail is not None
    ]
    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_ops_017_release_publish_exports_container_assets() -> None:
    """REQ-OPS-017: Release Publish must ship container quick-start assets and docs."""
    details = _collect_release_publish_container_details()
    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_ops_018_cli_release_binaries_and_install_script() -> None:
    """REQ-OPS-018: CLI release binaries and install path stay aligned."""
    details = _collect_cli_release_install_details()
    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_ops_018_release_publish_draft_job_checks_out_requested_target() -> (
    None
):
    """REQ-OPS-018: Release Publish draft job checks out the requested target."""
    _assert_release_publish_job_checks_out_requested_target(
        job_name="create-draft-release",
        release_step_name="Create or reuse draft release",
    )


def test_docs_req_ops_018_release_publish_finalize_job_checks_out_target() -> None:
    """REQ-OPS-018: Release Publish finalize job checks out the requested target."""
    _assert_release_publish_job_checks_out_requested_target(
        job_name="publish-release",
        release_step_name="Finalize GitHub release",
    )


def test_docs_req_ops_018_install_script_supports_prerelease_quick_start(
    tmp_path: Path,
) -> None:
    """REQ-OPS-018: Installer supports prerelease quick-start commands."""
    version = "0.0.1-beta.1"
    release_dir = _create_fake_cli_release_dir(tmp_path, version=version)
    home_dir = tmp_path / "home"
    work_dir = tmp_path / "work"
    install_dir = home_dir / ".local" / "bin"
    home_dir.mkdir()
    work_dir.mkdir()

    env = os.environ.copy()
    env.update(
        {
            "HOME": str(home_dir),
            "PATH": f"{install_dir}{os.pathsep}{env.get('PATH', '')}",
            "UGOITE_VERSION": version,
            "UGOITE_DOWNLOAD_BASE_URL": release_dir.as_uri(),
        },
    )
    installed_binary = install_dir / "ugoite"

    install_result = subprocess.run(
        ["/bin/bash", str(INSTALL_CLI_SCRIPT_PATH)],
        cwd=work_dir,
        env=env,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    if install_result.returncode != 0:
        message = (
            "install-ugoite-cli.sh should install an exact prerelease from a local "
            f"release mirror; stdout={install_result.stdout!r} "
            f"stderr={install_result.stderr!r}"
        )
        raise AssertionError(message)
    if not installed_binary.exists():
        message = "install-ugoite-cli.sh should install ugoite into ~/.local/bin"
        raise AssertionError(message)

    help_result = subprocess.run(
        [str(installed_binary), "--help"],
        cwd=work_dir,
        env=env,
        text=True,
        capture_output=True,
        timeout=10,
        check=False,
    )
    if help_result.returncode != 0 or "Ugoite CLI - Knowledge base management" not in (
        help_result.stdout
    ):
        message = (
            "installed prerelease CLI should answer --help; "
            f"stdout={help_result.stdout!r} stderr={help_result.stderr!r}"
        )
        raise AssertionError(message)

    spaces_dir = work_dir / "spaces"
    spaces_dir.mkdir()
    list_before_result = subprocess.run(
        [str(installed_binary), "space", "list", "--root", "./spaces"],
        cwd=work_dir,
        env=env,
        text=True,
        capture_output=True,
        timeout=10,
        check=False,
    )
    if list_before_result.returncode != 0:
        message = (
            "installed prerelease CLI should list spaces before create-space; "
            f"stdout={list_before_result.stdout!r} "
            f"stderr={list_before_result.stderr!r}"
        )
        raise AssertionError(message)
    if json.loads(list_before_result.stdout) != []:
        message = "space list should start empty for a fresh quick-start workspace"
        raise AssertionError(message)

    create_result = subprocess.run(
        [str(installed_binary), "create-space", "--root", "./spaces", "demo"],
        cwd=work_dir,
        env=env,
        text=True,
        capture_output=True,
        timeout=10,
        check=False,
    )
    if create_result.returncode != 0:
        message = (
            "installed prerelease CLI should create a documented quick-start space; "
            f"stdout={create_result.stdout!r} stderr={create_result.stderr!r}"
        )
        raise AssertionError(message)
    if json.loads(create_result.stdout) != {"created": True, "id": "demo"}:
        message = "create-space should report the created demo space"
        raise AssertionError(message)

    list_after_result = subprocess.run(
        [str(installed_binary), "space", "list", "--root", "./spaces"],
        cwd=work_dir,
        env=env,
        text=True,
        capture_output=True,
        timeout=10,
        check=False,
    )
    if list_after_result.returncode != 0:
        message = (
            "installed prerelease CLI should list the created demo space; "
            f"stdout={list_after_result.stdout!r} "
            f"stderr={list_after_result.stderr!r}"
        )
        raise AssertionError(message)
    if json.loads(list_after_result.stdout) != ["demo"]:
        message = "space list should include the created demo space"
        raise AssertionError(message)


def test_docs_req_ops_018_release_quick_start_smoke_script_validates_prerelease(
    tmp_path: Path,
) -> None:
    """REQ-OPS-018: Release quick-start smoke script validates a prerelease."""
    version = "0.0.1-beta.2"
    release_dir = _create_fake_cli_release_dir(tmp_path, version=version)
    work_dir = tmp_path / "quick-start-smoke"

    _assert_release_quick_start_smoke(
        tmp_path=tmp_path,
        version=version,
        release_dir=release_dir,
        install_script_path=INSTALL_CLI_SCRIPT_PATH,
        work_dir=work_dir,
    )


def test_docs_req_ops_018_platform_installer_asset_validates_prerelease(
    tmp_path: Path,
) -> None:
    """REQ-OPS-018: Platform installer asset validates a prerelease quick-start."""
    version = "0.0.1-beta.3"
    release_dir = _create_fake_cli_release_dir(tmp_path, version=version)
    work_dir = tmp_path / "quick-start-asset-smoke"
    target = _detect_install_cli_target()
    installer_asset_path = tmp_path / f"ugoite-v{version}-{target}.install.sh"

    render_result = subprocess.run(
        [
            "/bin/bash",
            str(RELEASE_INSTALLER_RENDERER_PATH),
            version,
            target,
            str(installer_asset_path),
        ],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        timeout=10,
        check=False,
    )
    if render_result.returncode != 0:
        message = (
            "render-cli-release-installer.sh should generate a target-specific "
            f"installer asset; stdout={render_result.stdout!r} "
            f"stderr={render_result.stderr!r}"
        )
        raise AssertionError(message)

    _assert_release_quick_start_smoke(
        tmp_path=tmp_path,
        version=version,
        release_dir=release_dir,
        install_script_path=installer_asset_path,
        work_dir=work_dir,
    )


def _assert_release_publish_job_checks_out_requested_target(
    *,
    job_name: str,
    release_step_name: str,
) -> None:
    """Ensure a release-publish job checks out the requested target."""
    workflow = _load_yaml_base_mapping(RELEASE_PUBLISH_WORKFLOW_PATH)
    jobs = workflow.get("jobs", {})
    if not isinstance(jobs, dict):
        message = "release-publish.yml must define jobs"
        raise TypeError(message)

    release_job = jobs.get(job_name)
    if not isinstance(release_job, dict):
        message = f"release-publish.yml must define jobs.{job_name}"
        raise TypeError(message)

    steps = release_job.get("steps", [])
    if not isinstance(steps, list):
        message = f"{job_name} steps must be a list"
        raise TypeError(message)

    checkout_index = next(
        (
            index
            for index, step in enumerate(steps)
            if isinstance(step, dict) and step.get("uses") == "actions/checkout@v6"
        ),
        None,
    )
    if checkout_index is None:
        message = f"{job_name} must check out the requested release target"
        raise AssertionError(message)

    checkout_step = steps[checkout_index]
    if not isinstance(checkout_step, dict):
        message = "checkout step must be a mapping"
        raise TypeError(message)

    with_block = checkout_step.get("with", {})
    if not isinstance(with_block, dict):
        message = "checkout step must define a with mapping"
        raise TypeError(message)

    configured_ref = str(with_block.get("ref", "")).strip()
    if configured_ref != REQUIRED_RELEASE_DRAFT_CHECKOUT_REF:
        message = (
            f"{job_name} checkout must use the requested target ref "
            f"(got {configured_ref!r})"
        )
        raise AssertionError(message)

    release_step_index = next(
        (
            index
            for index, step in enumerate(steps)
            if isinstance(step, dict) and step.get("name") == release_step_name
        ),
        None,
    )
    if release_step_index is None:
        message = f"{job_name} must define the {release_step_name!r} step"
        raise AssertionError(message)

    if checkout_index >= release_step_index:
        message = f"{job_name} must check out the target before {release_step_name}"
        raise AssertionError(message)


def test_docs_req_ops_019_mise_monorepo_config_roots_are_explicit() -> None:
    """REQ-OPS-019: root mise must declare explicit monorepo config roots."""
    root_mise = tomllib.loads(MISE_PATH.read_text(encoding="utf-8"))
    monorepo = root_mise.get("monorepo")
    if not isinstance(monorepo, dict):
        message = "root mise.toml must define [monorepo]"
        raise TypeError(message)

    config_roots = monorepo.get("config_roots")
    if not isinstance(config_roots, list):
        message = "root mise.toml [monorepo].config_roots must be a list"
        raise TypeError(message)
    if not all(isinstance(item, str) for item in config_roots):
        message = "root mise.toml [monorepo].config_roots must contain strings only"
        raise TypeError(message)

    root_set = {item for item in config_roots if item}
    discovered = {
        Path(path).parent.as_posix()
        for path in _discover_repo_paths("mise.toml")
        if "/" in path
    }

    missing_expected = sorted(EXPECTED_MONOREPO_CONFIG_ROOTS.difference(root_set))
    uncovered = sorted(discovered.difference(root_set))
    guide_text = CI_CD_SPEC_PATH.read_text(encoding="utf-8")
    missing_doc_fragments = sorted(
        fragment
        for fragment in REQUIRED_MONOREPO_DOC_FRAGMENTS
        if fragment not in guide_text
    )

    detail_candidates = (
        (
            bool(missing_expected),
            "root mise config_roots missing: " + ", ".join(missing_expected),
        ),
        (
            bool(uncovered),
            "root mise config_roots do not cover package mise files: "
            + ", ".join(uncovered),
        ),
        (
            bool(missing_doc_fragments),
            "ci-cd guide missing monorepo fragments: "
            + ", ".join(missing_doc_fragments),
        ),
    )
    details = [message for condition, message in detail_candidates if condition]
    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_ops_020_minimum_wasm_gates_and_boundaries_declared() -> None:
    """REQ-OPS-020: ugoite-minimum must keep WASM gates and boundary docs explicit."""
    pre_commit = _load_pre_commit_config()
    configured_hooks = _collect_pre_commit_hook_ids(pre_commit)
    missing_hooks = sorted(
        REQUIRED_MINIMUM_WASM_PRE_COMMIT_HOOKS.difference(configured_hooks),
    )

    workflow_step_names = _collect_workflow_step_names(RUST_CI_WORKFLOW_PATH)
    missing_ci_steps = sorted(
        REQUIRED_MINIMUM_WASM_CI_STEPS.difference(workflow_step_names),
    )

    minimum_readme = _read_required_text(
        REPO_ROOT / "ugoite-minimum" / "README.md",
        "ugoite-minimum README is missing at {path}; required by REQ-OPS-020.",
    )
    future_proofing = _read_required_text(
        REPO_ROOT / "docs" / "spec" / "architecture" / "future-proofing.md",
        "future-proofing.md is missing at {path}; required by REQ-OPS-020.",
    )

    missing_readme = sorted(
        fragment
        for fragment in REQUIRED_MINIMUM_BOUNDARY_README_FRAGMENTS
        if fragment not in minimum_readme
    )
    missing_doc = sorted(
        fragment
        for fragment in REQUIRED_MINIMUM_BOUNDARY_DOC_FRAGMENTS
        if fragment not in future_proofing
    )

    details: list[str] = []
    if missing_hooks:
        details.append(
            "pre-commit missing minimum wasm hooks: " + ", ".join(missing_hooks),
        )
    if missing_ci_steps:
        details.append(
            "rust-ci missing minimum wasm steps: " + ", ".join(missing_ci_steps),
        )
    if missing_readme:
        details.append(
            "ugoite-minimum README missing boundary fragments: "
            + ", ".join(missing_readme),
        )
    if missing_doc:
        details.append(
            "future-proofing.md missing minimum/core boundary fragments: "
            + ", ".join(missing_doc),
        )

    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_ops_021_frontend_coverage_gate_is_explicit() -> None:
    """REQ-OPS-021: Frontend 100% coverage must stay explicit in CI and root tests."""
    root_mise = tomllib.loads(MISE_PATH.read_text(encoding="utf-8"))
    root_runs = _get_task_run_commands(root_mise, "test")
    frontend_mise = tomllib.loads(FRONTEND_MISE_PATH.read_text(encoding="utf-8"))
    coverage_task = _load_mise_task_mapping(
        frontend_mise,
        task_name="test:coverage",
        path_label="frontend/mise.toml",
    )
    coverage_run = _load_task_run(
        coverage_task,
        task_label='frontend/mise.toml [tasks."test:coverage"]',
    )
    coverage_step_run = _find_workflow_step_run(
        FRONTEND_CI_WORKFLOW_PATH,
        job_name="ci",
        step_name=REQUIRED_FRONTEND_COVERAGE_STEP_NAME,
    )
    guide_text = CI_CD_SPEC_PATH.read_text(encoding="utf-8")
    missing_doc_fragments = sorted(
        fragment
        for fragment in REQUIRED_FRONTEND_COVERAGE_DOC_FRAGMENTS
        if fragment not in guide_text
    )

    detail_candidates = (
        (
            "mise run //frontend:test:coverage" not in root_runs,
            "root mise.toml tasks.test must run //frontend:test:coverage",
        ),
        (
            REQUIRED_FRONTEND_COVERAGE_COMMAND not in coverage_run,
            'frontend/mise.toml [tasks."test:coverage"] must run node vitest coverage',
        ),
        (
            coverage_step_run is None,
            "frontend-ci.yml must define the frontend coverage gate step",
        ),
        (
            coverage_step_run is not None
            and REQUIRED_FRONTEND_COVERAGE_COMMAND not in coverage_step_run,
            "frontend-ci.yml coverage gate must run node vitest coverage",
        ),
        (
            bool(missing_doc_fragments),
            "ci-cd guide missing frontend coverage fragments: "
            + ", ".join(missing_doc_fragments),
        ),
    )
    details = [message for condition, message in detail_candidates if condition]
    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_ops_022_e2e_ci_is_tiered_for_prs_and_full_on_merge_queue() -> None:
    """REQ-OPS-022: E2E CI must tier PRs while keeping merge-queue coverage full."""
    workflow = _load_yaml_base_mapping(E2E_CI_WORKFLOW_PATH)
    jobs = workflow.get("jobs", {})
    if not isinstance(jobs, dict):
        message = "e2e-ci.yml must define jobs"
        raise TypeError(message)

    required_jobs = {
        "build-images": jobs.get("build-images"),
        "select-tier": jobs.get("select-tier"),
        "e2e": jobs.get("e2e"),
    }
    missing_jobs = sorted(
        name for name, value in required_jobs.items() if not isinstance(value, dict)
    )

    workflow_text = E2E_CI_WORKFLOW_PATH.read_text(encoding="utf-8")
    ci_cd_text = CI_CD_SPEC_PATH.read_text(encoding="utf-8")

    workflow_fragments = {
        'event_name in {"merge_group", "push"}',
        "docs/**",
        "docsite/**",
        (
            "bash e2e/scripts/run-e2e-compose.sh "
            '"${{ needs.select-tier.outputs.test_type }}"'
        ),
        'run-e2e-compose.sh "${{ needs.select-tier.outputs.test_type }}"',
    }
    missing_workflow_fragments = sorted(
        fragment for fragment in workflow_fragments if fragment not in workflow_text
    )

    doc_fragments = {
        "Pull requests only",
        "`merge_group` and",
        "pushes to `main` always run the full compose-backed suite.",
        "pull_request with docs/docsite-only paths => smoke",
        "| E2E Tests | `.github/workflows/e2e-ci.yml` | Push, PR, merge queue |",
    }
    missing_doc_fragments = sorted(
        fragment for fragment in doc_fragments if fragment not in ci_cd_text
    )

    details: list[str] = []
    if missing_jobs:
        details.append("e2e-ci missing jobs: " + ", ".join(missing_jobs))
    if missing_workflow_fragments:
        details.append(
            "e2e-ci missing tiering fragments: "
            + ", ".join(missing_workflow_fragments),
        )
    if missing_doc_fragments:
        details.append(
            "ci-cd guide missing E2E tiering fragments: "
            + ", ".join(missing_doc_fragments),
        )
    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_ops_023_public_package_stays_separate_from_private_tooling() -> None:
    """REQ-OPS-023: Public installer package stays separate from private tooling."""
    root_package = _load_json_mapping(ROOT_PACKAGE_JSON_PATH)
    public_package = _load_json_mapping(PUBLIC_PACKAGE_JSON_PATH)
    manifest = _load_json_mapping(RELEASE_MANIFEST_PATH)
    release_config = _load_json_mapping(RELEASE_CONFIG_PATH)
    ci_cd_text = CI_CD_SPEC_PATH.read_text(encoding="utf-8")
    public_readme = _read_required_text(
        PUBLIC_PACKAGE_README_PATH,
        "public package README is missing at {path}; required by REQ-OPS-023.",
    )
    public_license = _read_required_text(
        PUBLIC_PACKAGE_LICENSE_PATH,
        "public package LICENSE is missing at {path}; required by REQ-OPS-023.",
    )
    public_installer = _read_required_text(
        PUBLIC_PACKAGE_INSTALLER_PATH,
        "public package installer is missing at {path}; required by REQ-OPS-023.",
    )

    packages = release_config.get("packages")
    if not isinstance(packages, dict):
        message = "release-please-config.json must define packages"
        raise TypeError(message)
    release_entry = packages.get("packages/ugoite")
    if not isinstance(release_entry, dict):
        message = 'release-please-config.json must define packages."packages/ugoite"'
        raise TypeError(message)

    npm_executable = shutil.which("npm")
    if not npm_executable:
        message = "npm must be available to validate packages/ugoite"
        raise AssertionError(message)

    pack_result = subprocess.run(
        [npm_executable, "pack", "--json", "--dry-run"],
        cwd=PUBLIC_PACKAGE_DIR,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    packed_files: set[str] = set()
    if pack_result.returncode == 0:
        pack_payload = json.loads(pack_result.stdout or "[]")
        if isinstance(pack_payload, list) and pack_payload:
            files = pack_payload[0].get("files", [])
            if isinstance(files, list):
                packed_files = {
                    str(item.get("path"))
                    for item in files
                    if isinstance(item, dict) and isinstance(item.get("path"), str)
                }

    script_url_result = subprocess.run(
        ["/bin/bash", str(PUBLIC_PACKAGE_INSTALLER_PATH), "--print-script-url"],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        timeout=10,
        check=False,
    )

    public_version = str(public_package.get("version", ""))
    expected_script_url = (
        "https://raw.githubusercontent.com/ugoite/ugoite/"
        f"v{public_version}/scripts/install-ugoite-cli.sh"
    )
    missing_doc_fragments = sorted(
        fragment
        for fragment in REQUIRED_PUBLIC_PACKAGE_DOC_FRAGMENTS
        if fragment not in ci_cd_text
    )
    missing_readme_fragments = sorted(
        fragment
        for fragment in REQUIRED_PUBLIC_PACKAGE_README_FRAGMENTS
        if fragment not in public_readme
    )
    missing_installer_fragments = sorted(
        fragment
        for fragment in REQUIRED_PUBLIC_PACKAGE_INSTALLER_FRAGMENTS
        if fragment not in public_installer
    )
    required_pack_files = {
        "package.json",
        "README.md",
        "LICENSE",
        "bin/ugoite-install",
    }
    missing_pack_files = sorted(required_pack_files.difference(packed_files))

    detail_candidates = (
        (
            root_package.get("private") is not True,
            "root package.json must stay private tooling",
        ),
        (
            str(root_package.get("name", "")).strip() != "ugoite-release-tooling",
            "root package.json must stay named ugoite-release-tooling",
        ),
        (
            public_package.get("private") is True,
            "packages/ugoite/package.json must not be private",
        ),
        (
            str(public_package.get("name", "")).strip() != "ugoite",
            "packages/ugoite/package.json must define name=ugoite",
        ),
        (
            str(manifest.get("packages/ugoite", "")).strip() != public_version,
            "release manifest must track packages/ugoite package.json version",
        ),
        (
            str(release_entry.get("package-name", "")).strip() != "ugoite",
            "release-please packages/ugoite entry must define package-name=ugoite",
        ),
        (
            not isinstance(public_package.get("files"), list),
            "packages/ugoite/package.json must define a files allowlist",
        ),
        (
            not isinstance(public_package.get("bin"), dict)
            or str(public_package["bin"].get("ugoite-install", "")).strip()
            != "./bin/ugoite-install",
            "packages/ugoite/package.json must expose bin.ugoite-install",
        ),
        (
            not isinstance(public_package.get("publishConfig"), dict)
            or str(public_package["publishConfig"].get("access", "")).strip()
            != "public",
            "packages/ugoite/package.json must set publishConfig.access=public",
        ),
        (
            not isinstance(public_package.get("repository"), dict)
            or str(public_package["repository"].get("directory", "")).strip()
            != "packages/ugoite",
            "packages/ugoite/package.json repository.directory must be packages/ugoite",
        ),
        (
            not isinstance(public_package.get("bugs"), dict)
            or not str(public_package["bugs"].get("url", "")).strip(),
            "packages/ugoite/package.json must define bugs.url",
        ),
        (
            str(public_package.get("license", "")).strip() != "MIT",
            "packages/ugoite/package.json must define license=MIT",
        ),
        (
            str(public_license.splitlines()[0]).strip() != "MIT License",
            "packages/ugoite/LICENSE must preserve the MIT license text",
        ),
        (
            bool(missing_doc_fragments),
            "ci-cd guide missing public package fragments: "
            + ", ".join(missing_doc_fragments),
        ),
        (
            bool(missing_readme_fragments),
            "packages/ugoite/README.md missing fragments: "
            + ", ".join(missing_readme_fragments),
        ),
        (
            bool(missing_installer_fragments),
            "packages/ugoite/bin/ugoite-install missing fragments: "
            + ", ".join(missing_installer_fragments),
        ),
        (
            pack_result.returncode != 0,
            "npm pack --dry-run failed for packages/ugoite: "
            + pack_result.stderr.strip(),
        ),
        (
            bool(missing_pack_files),
            "packages/ugoite pack output missing files: "
            + ", ".join(missing_pack_files),
        ),
        (
            script_url_result.returncode != 0,
            "ugoite-install --print-script-url failed: "
            + script_url_result.stderr.strip(),
        ),
        (
            script_url_result.returncode == 0
            and script_url_result.stdout.strip() != expected_script_url,
            (
                "ugoite-install --print-script-url must target the matching versioned "
                f"installer script (got {script_url_result.stdout.strip()!r})"
            ),
        ),
    )
    details = [message for condition, message in detail_candidates if condition]
    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_ops_024_docsite_coverage_gate_is_explicit() -> None:
    """REQ-OPS-024: Docsite 100% coverage must stay explicit in CI and root tests."""
    root_mise = tomllib.loads(MISE_PATH.read_text(encoding="utf-8"))
    root_runs = _get_task_run_commands(root_mise, "test")
    docsite_mise = tomllib.loads(DOCSITE_MISE_PATH.read_text(encoding="utf-8"))
    coverage_task = _load_mise_task_mapping(
        docsite_mise,
        task_name="test:coverage",
        path_label="docsite/mise.toml",
    )
    coverage_run = _load_task_run(
        coverage_task,
        task_label='docsite/mise.toml [tasks."test:coverage"]',
    )
    coverage_step_run = _find_workflow_step_run(
        DOCSITE_CI_WORKFLOW_PATH,
        job_name="ci",
        step_name=REQUIRED_DOCSITE_COVERAGE_STEP_NAME,
    )
    guide_text = CI_CD_SPEC_PATH.read_text(encoding="utf-8")
    missing_doc_fragments = sorted(
        fragment
        for fragment in REQUIRED_DOCSITE_COVERAGE_DOC_FRAGMENTS
        if fragment not in guide_text
    )

    detail_candidates = (
        (
            "mise run //docsite:test:coverage" not in root_runs,
            "root mise.toml tasks.test must run //docsite:test:coverage",
        ),
        (
            REQUIRED_DOCSITE_COVERAGE_COMMAND not in coverage_run,
            'docsite/mise.toml [tasks."test:coverage"] must run node vitest coverage',
        ),
        (
            coverage_step_run is None,
            "docsite-ci.yml must define the docsite coverage gate step",
        ),
        (
            coverage_step_run is not None
            and REQUIRED_DOCSITE_COVERAGE_COMMAND not in coverage_step_run,
            "docsite-ci.yml coverage gate must run node vitest coverage",
        ),
        (
            bool(missing_doc_fragments),
            "ci-cd guide missing docsite coverage fragments: "
            + ", ".join(missing_doc_fragments),
        ),
    )
    details = [message for condition, message in detail_candidates if condition]
    if details:
        raise AssertionError("; ".join(details))


def _collect_all_tests_status_details() -> list[str]:
    workflow_text = ALL_TESTS_WORKFLOW_PATH.read_text(encoding="utf-8")
    workflow = _load_yaml_base_mapping(ALL_TESTS_WORKFLOW_PATH)
    jobs = workflow.get("jobs", {})
    if not isinstance(jobs, dict):
        message = "all-tests-ci.yml must define jobs"
        raise TypeError(message)

    all_tests_job = jobs.get("all-tests-check")
    if not isinstance(all_tests_job, dict):
        message = "all-tests-ci.yml must define jobs.all-tests-check"
        raise TypeError(message)

    steps = all_tests_job.get("steps", [])
    if not isinstance(steps, list):
        message = "all-tests-ci.yml jobs.all-tests-check.steps must be a list"
        raise TypeError(message)

    wait_step = next(
        (
            step
            for step in steps
            if isinstance(step, dict)
            and step.get("name") == "Wait for curated workflows"
        ),
        None,
    )
    if wait_step is None:
        return ["all-tests-ci missing curated wait step"]

    excluded_workflows, curated_workflows = _collect_all_tests_workflow_env(wait_step)
    missing_exclusions = _missing_all_tests_workflows(
        excluded_workflows,
        REQUIRED_ALL_TESTS_EXCLUDED_WORKFLOWS,
    )
    missing_curated = _missing_all_tests_workflows(
        curated_workflows,
        REQUIRED_ALL_TESTS_CURATED_WORKFLOWS,
    )
    guide_text = CI_CD_SPEC_PATH.read_text(encoding="utf-8")
    missing_doc_fragments = sorted(
        fragment
        for fragment in REQUIRED_ALL_TESTS_DOC_FRAGMENTS
        if fragment not in guide_text
    )

    details: list[str] = []
    if "int128/wait-for-workflows-action@v1" in workflow_text:
        details.append(
            "all-tests-ci must not use deprecated wait-for-workflows-action@v1",
        )
    if re.search(r"""\[\s*["']gh["']\s*,\s*["']api["']\s*,""", workflow_text) is None:
        details.append("all-tests-ci must poll workflow status via gh api")
    if missing_exclusions:
        details.append(
            "all-tests-ci exclude-workflow-names missing: "
            + ", ".join(missing_exclusions),
        )
    if missing_curated:
        details.append(
            "all-tests-ci curated workflow list missing: " + ", ".join(missing_curated),
        )
    if missing_doc_fragments:
        details.append(
            "ci-cd guide missing All Tests Status fragments: "
            + ", ".join(missing_doc_fragments),
        )
    return details


def _collect_all_tests_workflow_env(wait_step: dict[object, object]) -> tuple[str, str]:
    env_block = wait_step.get("env", {})
    if not isinstance(env_block, dict):
        message = "all-tests-ci curated wait step must define an env mapping"
        raise TypeError(message)

    excluded_workflows = env_block.get("EXCLUDED_WORKFLOW_NAMES", "")
    curated_workflows = env_block.get("CURATED_WORKFLOW_NAMES", "")
    if not isinstance(excluded_workflows, str) or not isinstance(
        curated_workflows,
        str,
    ):
        message = "all-tests-ci workflow name env vars must be multiline strings"
        raise TypeError(message)
    return excluded_workflows, curated_workflows


def _missing_all_tests_workflows(
    configured: str,
    required: set[str],
) -> list[str]:
    return sorted(
        workflow_name for workflow_name in required if workflow_name not in configured
    )


def _collect_release_ci_requirement_details() -> list[str]:
    workflow_text = RELEASE_CI_WORKFLOW_PATH.read_text(encoding="utf-8")
    workflow = yaml.safe_load(workflow_text) or {}
    if not isinstance(workflow, dict):
        message = "release-ci.yml must be a YAML mapping"
        raise TypeError(message)

    permissions = workflow.get("permissions")
    if not isinstance(permissions, dict):
        message = "release-ci.yml must define top-level permissions"
        raise TypeError(message)

    missing_permissions = [
        f"{name}={expected}"
        for name, expected in REQUIRED_RELEASE_CI_PERMISSIONS.items()
        if str(permissions.get(name)) != expected
    ]

    release_ci_steps = _collect_workflow_step_names(RELEASE_CI_WORKFLOW_PATH)
    missing_steps = sorted(
        {
            "Print release auth path",
            "Run release-please",
            "Skip release-please without dedicated token",
        }.difference(release_ci_steps),
    )
    missing_token_fragments = sorted(
        fragment
        for fragment in REQUIRED_RELEASE_CI_TOKEN_FRAGMENTS
        if fragment not in workflow_text
    )

    release_config = _load_json_mapping(RELEASE_CONFIG_PATH)
    manifest = _load_json_mapping(RELEASE_MANIFEST_PATH)
    package_data = _load_json_mapping(PUBLIC_PACKAGE_JSON_PATH)
    bootstrap_sha = str(release_config.get("bootstrap-sha", "")).strip()
    manifest_version = str(manifest.get("packages/ugoite", ""))
    package_version = str(package_data.get("version", ""))

    guide_text = CI_CD_SPEC_PATH.read_text(encoding="utf-8")
    missing_doc_fragments = sorted(
        fragment
        for fragment in REQUIRED_RELEASE_CI_DOC_FRAGMENTS
        if fragment not in guide_text
    )

    detail_candidates = (
        (
            bool(missing_permissions),
            "release-ci permissions mismatch: " + ", ".join(missing_permissions),
        ),
        (
            bool(missing_steps),
            "release-ci missing steps: " + ", ".join(missing_steps),
        ),
        (
            bool(missing_token_fragments),
            "release-ci missing token fragments: " + ", ".join(missing_token_fragments),
        ),
        (
            not bootstrap_sha,
            "release-please-config.json must define non-empty bootstrap-sha",
        ),
        (
            manifest_version != "0.0.1",
            f"release manifest must start at 0.0.1 (got {manifest_version!r})",
        ),
        (
            package_version != "0.0.1",
            (
                "packages/ugoite/package.json version must start at 0.0.1 "
                f"(got {package_version!r})"
            ),
        ),
        (
            bool(missing_doc_fragments),
            "ci-cd guide missing fragments: " + ", ".join(missing_doc_fragments),
        ),
    )
    return [message for condition, message in detail_candidates if condition]


def _collect_cli_release_install_details() -> list[str]:
    workflow_text = RELEASE_PUBLISH_WORKFLOW_PATH.read_text(encoding="utf-8")
    cli_release_workflow_text = CLI_RELEASE_WORKFLOW_PATH.read_text(encoding="utf-8")
    gitignore_text = ROOT_GITIGNORE_PATH.read_text(encoding="utf-8")
    renderer_text = _read_required_text(
        RELEASE_INSTALLER_RENDERER_PATH,
        "scripts/render-cli-release-installer.sh is missing at {path}; "
        "required by REQ-OPS-018.",
    )
    workflow = _load_yaml_base_mapping(RELEASE_PUBLISH_WORKFLOW_PATH)
    jobs = workflow.get("jobs", {})
    if not isinstance(jobs, dict):
        message = "release-publish.yml must define jobs"
        raise TypeError(message)

    create_draft_job = jobs.get("create-draft-release")
    publish_cli_job = jobs.get("publish-cli-binaries")
    publish_release_job = jobs.get("publish-release")
    if not isinstance(create_draft_job, dict):
        message = "release-publish.yml must define jobs.create-draft-release"
        raise TypeError(message)
    if not isinstance(publish_cli_job, dict):
        message = "release-publish.yml must define jobs.publish-cli-binaries"
        raise TypeError(message)
    if not isinstance(publish_release_job, dict):
        message = "release-publish.yml must define jobs.publish-release"
        raise TypeError(message)

    publish_release_needs = publish_release_job.get("needs", [])
    if isinstance(publish_release_needs, str):
        publish_release_needs = [publish_release_needs]
    if not isinstance(publish_release_needs, list):
        message = (
            "release-publish.yml jobs.publish-release.needs must be a string or list"
        )
        raise TypeError(message)

    missing_release_publish = _missing_required_fragments(
        workflow_text,
        REQUIRED_RELEASE_PUBLISH_CLI_FRAGMENTS,
    )
    missing_cli_release = _missing_required_fragments(
        cli_release_workflow_text,
        REQUIRED_CLI_RELEASE_WORKFLOW_FRAGMENTS,
    )
    forbidden_cli_release_runners = sorted(
        fragment
        for fragment in FORBIDDEN_CLI_RELEASE_RUNNER_FRAGMENTS
        if fragment in cli_release_workflow_text
    )
    missing_install_script = _missing_required_fragments(
        INSTALL_CLI_SCRIPT_PATH.read_text(encoding="utf-8"),
        REQUIRED_INSTALL_CLI_SCRIPT_FRAGMENTS,
    )
    missing_renderer = _missing_required_fragments(
        renderer_text,
        REQUIRED_RELEASE_INSTALLER_RENDERER_FRAGMENTS,
    )
    missing_readme = _missing_required_fragments(
        README_PATH.read_text(encoding="utf-8"),
        REQUIRED_CLI_README_FRAGMENTS,
    )
    missing_cli_guide = _missing_required_fragments(
        CLI_GUIDE_PATH.read_text(encoding="utf-8"),
        REQUIRED_CLI_GUIDE_FRAGMENTS,
    )
    missing_ci_cd = _missing_required_fragments(
        CI_CD_SPEC_PATH.read_text(encoding="utf-8"),
        REQUIRED_CLI_CICD_FRAGMENTS,
    )

    detail_candidates = (
        (
            str(publish_cli_job.get("uses", "")).strip()
            != "./.github/workflows/cli-release-binaries.yml",
            "release-publish must delegate CLI binaries to cli-release-binaries.yml",
        ),
        (
            "publish-cli-binaries" not in publish_release_needs,
            "release-publish publish-release job must depend on publish-cli-binaries",
        ),
        (
            "create-draft-release" not in publish_release_needs,
            "release-publish publish-release job must depend on create-draft-release",
        ),
        (
            bool(missing_release_publish),
            "release-publish missing CLI release fragments: "
            + ", ".join(missing_release_publish),
        ),
        (
            bool(missing_cli_release),
            "cli-release-binaries workflow missing fragments: "
            + ", ".join(missing_cli_release),
        ),
        (
            bool(forbidden_cli_release_runners),
            "cli-release-binaries workflow still references forbidden runners: "
            + ", ".join(forbidden_cli_release_runners),
        ),
        (
            not ROOT_CARGO_LOCK_PATH.exists(),
            "repository root must commit Cargo.lock for clean-checkout release builds",
        ),
        (
            "/Cargo.lock" in gitignore_text,
            ".gitignore must not ignore the root Cargo.lock used by release builds",
        ),
        (
            bool(missing_install_script),
            "install-ugoite-cli.sh missing fragments: "
            + ", ".join(missing_install_script),
        ),
        (
            bool(missing_renderer),
            "render-cli-release-installer.sh missing fragments: "
            + ", ".join(missing_renderer),
        ),
        (
            bool(missing_readme),
            "README missing CLI install fragments: " + ", ".join(missing_readme),
        ),
        (
            bool(missing_cli_guide),
            "cli.md missing release install fragments: " + ", ".join(missing_cli_guide),
        ),
        (
            bool(missing_ci_cd),
            "ci-cd guide missing CLI release fragments: " + ", ".join(missing_ci_cd),
        ),
    )
    return [message for condition, message in detail_candidates if condition]


def _collect_release_publish_container_details() -> list[str]:
    workflow_text = RELEASE_PUBLISH_WORKFLOW_PATH.read_text(encoding="utf-8")
    workflow = _load_yaml_base_mapping(RELEASE_PUBLISH_WORKFLOW_PATH)
    permissions = workflow.get("permissions")
    if not isinstance(permissions, dict):
        message = "release-publish.yml must define top-level permissions"
        raise TypeError(message)

    missing_permissions = [
        f"{name}={expected}"
        for name, expected in REQUIRED_RELEASE_PUBLISH_PERMISSIONS.items()
        if str(permissions.get(name)) != expected
    ]

    (
        publish_images_uses,
        export_images_uses,
        publish_images_permissions,
        export_images_permissions,
        release_needs,
    ) = _collect_release_publish_jobs(workflow)

    missing_workflow_fragments = sorted(
        fragment
        for fragment in REQUIRED_RELEASE_PUBLISH_WORKFLOW_FRAGMENTS
        if fragment not in workflow_text
    )

    missing_reusable_fragments = _missing_required_fragments(
        DOCKER_IMAGES_REUSABLE_WORKFLOW_PATH.read_text(encoding="utf-8"),
        REQUIRED_DOCKER_IMAGES_REUSABLE_FRAGMENTS,
    )
    missing_readme_fragments = _missing_required_fragments(
        README_PATH.read_text(encoding="utf-8"),
        REQUIRED_RELEASE_QUICKSTART_README_FRAGMENTS,
    )
    missing_guide_fragments = _missing_required_fragments(
        CONTAINER_QUICKSTART_GUIDE_PATH.read_text(encoding="utf-8"),
        REQUIRED_RELEASE_QUICKSTART_GUIDE_FRAGMENTS,
    )
    missing_compose_fragments = _missing_required_fragments(
        RELEASE_COMPOSE_PATH.read_text(encoding="utf-8"),
        REQUIRED_RELEASE_COMPOSE_FRAGMENTS,
    )
    missing_ci_cd_fragments = _missing_required_fragments(
        CI_CD_SPEC_PATH.read_text(encoding="utf-8"),
        REQUIRED_RELEASE_QUICKSTART_CICD_FRAGMENTS,
    )

    detail_candidates = (
        (
            bool(missing_permissions),
            "release-publish permissions mismatch: " + ", ".join(missing_permissions),
        ),
        (
            publish_images_uses != "./.github/workflows/docker-images.yml",
            "release-publish must delegate image builds to docker-images.yml",
        ),
        (
            export_images_uses != "./.github/workflows/docker-images.yml",
            "release-publish must delegate image archive export to docker-images.yml",
        ),
        (
            str(publish_images_permissions.get("packages")) != "write",
            "release-publish publish-images job must allow packages: write",
        ),
        (
            str(export_images_permissions.get("packages")) != "write",
            "release-publish export-release-image-archives job must allow "
            "packages: write",
        ),
        (
            "publish-images" not in release_needs,
            "release-publish publish-release job must depend on publish-images",
        ),
        (
            "export-release-image-archives" not in release_needs,
            "release-publish publish-release job must depend on "
            "export-release-image-archives",
        ),
        (
            bool(missing_workflow_fragments),
            "release-publish missing workflow fragments: "
            + ", ".join(missing_workflow_fragments),
        ),
        (
            bool(missing_reusable_fragments),
            "docker-images reusable workflow missing fragments: "
            + ", ".join(missing_reusable_fragments),
        ),
        (
            bool(missing_readme_fragments),
            "README missing release quick-start fragments: "
            + ", ".join(missing_readme_fragments),
        ),
        (
            bool(missing_guide_fragments),
            "container-quickstart.md missing fragments: "
            + ", ".join(missing_guide_fragments),
        ),
        (
            bool(missing_compose_fragments),
            "docker-compose.release.yaml missing fragments: "
            + ", ".join(missing_compose_fragments),
        ),
        (
            bool(missing_ci_cd_fragments),
            "ci-cd guide missing release container fragments: "
            + ", ".join(missing_ci_cd_fragments),
        ),
    )
    return [message for condition, message in detail_candidates if condition]


def _assert_release_quick_start_smoke(
    *,
    tmp_path: Path,
    version: str,
    release_dir: Path,
    install_script_path: Path,
    work_dir: Path,
) -> None:
    env = os.environ.copy()
    env.update(
        {
            "UGOITE_VERSION": version,
            "UGOITE_DOWNLOAD_BASE_URL": release_dir.as_uri(),
            "UGOITE_INSTALL_SCRIPT_PATH": str(install_script_path),
            "UGOITE_QUICKSTART_WORKDIR": str(work_dir),
        },
    )

    smoke_result = subprocess.run(
        ["/bin/bash", str(RELEASE_CLI_QUICKSTART_SCRIPT_PATH)],
        cwd=tmp_path,
        env=env,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    if smoke_result.returncode != 0:
        message = (
            "release quick-start smoke script should validate a published "
            f"prerelease flow; stdout={smoke_result.stdout!r} "
            f"stderr={smoke_result.stderr!r}"
        )
        raise AssertionError(message)
    if "Quick-start smoke test passed" not in smoke_result.stderr:
        message = (
            "release quick-start smoke script should report a successful "
            f"verification; stderr={smoke_result.stderr!r}"
        )
        raise AssertionError(message)

    installed_binary = work_dir / "home" / ".local" / "bin" / "ugoite"
    if not installed_binary.exists():
        message = "release quick-start smoke script should install ugoite in workdir"
        raise AssertionError(message)

    created_space_dir = work_dir / "work" / "spaces" / "demo"
    if not created_space_dir.is_dir():
        message = "release quick-start smoke script should create the demo space"
        raise AssertionError(message)


def _create_fake_cli_release_dir(tmp_path: Path, *, version: str) -> Path:
    target = _detect_install_cli_target()
    release_tag = f"v{version}"
    release_dir = tmp_path / "release"
    archive_name = f"ugoite-{release_tag}-{target}.tar.gz"
    archive_path = release_dir / archive_name
    checksum_path = release_dir / f"{archive_name}.sha256"
    binary_path = tmp_path / "ugoite"

    release_dir.mkdir()
    binary_path.write_text(_fake_quick_start_cli_script(), encoding="utf-8")
    binary_path.chmod(0o755)

    with tarfile.open(archive_path, "w:gz") as archive:
        archive.add(binary_path, arcname="ugoite")

    checksum = sha256(archive_path.read_bytes()).hexdigest()
    checksum_path.write_text(f"{checksum}  {archive_name}\n", encoding="utf-8")
    return release_dir


def _detect_install_cli_target() -> str:
    sysname = os.uname().sysname
    machine = os.uname().machine

    if sysname == "Linux":
        if machine == "x86_64":
            return "x86_64-unknown-linux-gnu"
        if machine in {"arm64", "aarch64"}:
            return "aarch64-unknown-linux-gnu"
        message = f"Unsupported Linux architecture for install script test: {machine}"
        raise AssertionError(message)

    if sysname == "Darwin":
        if machine == "x86_64":
            return "x86_64-apple-darwin"
        if machine in {"arm64", "aarch64"}:
            return "aarch64-apple-darwin"
        message = f"Unsupported macOS architecture for install script test: {machine}"
        raise AssertionError(message)

    message = f"Unsupported operating system for install script test: {sysname}"
    raise AssertionError(message)


def _fake_quick_start_cli_script() -> str:
    return textwrap.dedent(
        """\
        #!/usr/bin/env bash
        set -euo pipefail

        if [ "$#" -eq 0 ] || [ "${1:-}" = "--help" ] || [ "${1:-}" = "help" ]; then
          cat <<'EOF'
        Ugoite CLI - Knowledge base management

        Usage: ugoite <COMMAND>
        EOF
          exit 0
        fi

        if [ "${1:-}" = "space" ] && [ "${2:-}" = "list" ]; then
          shift 2
          root=""
          while [ "$#" -gt 0 ]; do
            case "$1" in
              --root)
                root="$2"
                shift 2
                ;;
              *)
                printf 'unsupported argument: %s\\n' "$1" >&2
                exit 1
                ;;
            esac
          done
          python - "$root" <<'PY'
        import json
        import sys
        from pathlib import Path

        root = Path(sys.argv[1])
        root.mkdir(parents=True, exist_ok=True)
        spaces = sorted(path.name for path in root.iterdir() if path.is_dir())
        json.dump(spaces, sys.stdout, indent=2)
        sys.stdout.write("\\n")
        PY
          exit 0
        fi

        if [ "${1:-}" = "create-space" ]; then
          shift
          root=""
          space_id=""
          while [ "$#" -gt 0 ]; do
            case "$1" in
              --root)
                root="$2"
                shift 2
                ;;
              *)
                space_id="$1"
                shift
                ;;
            esac
          done
          python - "$root" "$space_id" <<'PY'
        import json
        import sys
        from pathlib import Path

        root = Path(sys.argv[1])
        space_id = sys.argv[2]
        root.mkdir(parents=True, exist_ok=True)
        (root / space_id).mkdir(parents=True, exist_ok=True)
        json.dump({"created": True, "id": space_id}, sys.stdout, indent=2)
        sys.stdout.write("\\n")
        PY
          exit 0
        fi

        printf 'unsupported command: %s\\n' "$*" >&2
        exit 1
        """,
    )


def _collect_release_publish_jobs(
    workflow: dict[object, object],
) -> tuple[str, str, dict[object, object], dict[object, object], list[object]]:
    jobs = workflow.get("jobs", {})
    if not isinstance(jobs, dict):
        message = "release-publish.yml must define jobs"
        raise TypeError(message)

    publish_images_job = jobs.get("publish-images")
    export_images_job = jobs.get("export-release-image-archives")
    publish_release_job = jobs.get("publish-release")
    if not isinstance(publish_images_job, dict):
        message = "release-publish.yml must define jobs.publish-images"
        raise TypeError(message)
    if not isinstance(export_images_job, dict):
        message = "release-publish.yml must define jobs.export-release-image-archives"
        raise TypeError(message)
    if not isinstance(publish_release_job, dict):
        message = "release-publish.yml must define jobs.publish-release"
        raise TypeError(message)

    publish_images_uses = str(publish_images_job.get("uses", "")).strip()
    export_images_uses = str(export_images_job.get("uses", "")).strip()
    publish_images_permissions = publish_images_job.get("permissions", {})
    export_images_permissions = export_images_job.get("permissions", {})
    if not isinstance(publish_images_permissions, dict):
        message = (
            "release-publish.yml jobs.publish-images.permissions must be a mapping"
        )
        raise TypeError(message)
    if not isinstance(export_images_permissions, dict):
        message = (
            "release-publish.yml jobs.export-release-image-archives.permissions "
            "must be a mapping"
        )
        raise TypeError(message)
    release_needs = publish_release_job.get("needs", [])
    if isinstance(release_needs, str):
        release_needs = [release_needs]
    if not isinstance(release_needs, list):
        message = (
            "release-publish.yml jobs.publish-release.needs must be a string or list"
        )
        raise TypeError(message)
    return (
        publish_images_uses,
        export_images_uses,
        publish_images_permissions,
        export_images_permissions,
        release_needs,
    )


def _missing_required_fragments(text: str, required_fragments: set[str]) -> list[str]:
    return sorted(fragment for fragment in required_fragments if fragment not in text)


def _load_workflow(workflow_path: Path) -> dict[str, object]:
    workflow_text = workflow_path.read_text(encoding="utf-8")
    workflow = yaml.safe_load(workflow_text)
    if isinstance(workflow, dict):
        return workflow
    return {}


def _load_pre_commit_config() -> dict[str, object]:
    pre_commit_text = PRE_COMMIT_CONFIG_PATH.read_text(encoding="utf-8")
    pre_commit = yaml.safe_load(pre_commit_text)
    if isinstance(pre_commit, dict):
        return pre_commit
    message = ".pre-commit-config.yaml must be a YAML mapping"
    raise TypeError(message)


def _read_required_text(path: Path, missing_message: str) -> str:
    if not path.exists():
        message = missing_message.format(path=path)
        raise AssertionError(message)
    return path.read_text(encoding="utf-8")


def _load_yaml_base_mapping(path: Path) -> dict[str, object]:
    document = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
    if isinstance(document, dict):
        if "on" not in document and True in document:
            document["on"] = document.pop(True)
        return document
    message = f"{path.relative_to(REPO_ROOT)} must be a YAML mapping"
    raise TypeError(message)


def _load_json_mapping(path: Path) -> dict[str, object]:
    data = json.loads(path.read_text(encoding="utf-8"))
    if isinstance(data, dict):
        return data
    message = f"{path.name} must be a JSON mapping"
    raise TypeError(message)


def _collect_pre_commit_hooks(
    config: dict[str, object],
) -> dict[str, dict[str, object]]:
    repos = config.get("repos", [])
    if not isinstance(repos, list):
        return {}
    hooks_by_id: dict[str, dict[str, object]] = {}
    for repo in repos:
        if not isinstance(repo, dict):
            continue
        hooks = repo.get("hooks", [])
        if not isinstance(hooks, list):
            continue
        for hook in hooks:
            if not isinstance(hook, dict):
                continue
            hook_id = hook.get("id")
            if isinstance(hook_id, str) and hook_id:
                hooks_by_id[hook_id] = hook
    return hooks_by_id


def _collect_pre_commit_hook_ids(config: dict[str, object]) -> set[str]:
    return set(_collect_pre_commit_hooks(config))


def _get_env_value(config: dict[str, object], key: str) -> str | None:
    env = config.get("env", {})
    if not isinstance(env, dict):
        return None
    value = env.get(key)
    return value if isinstance(value, str) else None


def _get_task_run_commands(config: dict[str, object], task_name: str) -> list[str]:
    tasks = config.get("tasks", {})
    if not isinstance(tasks, dict):
        return []
    task = tasks.get(task_name)
    if not isinstance(task, dict):
        return []
    run = task.get("run")
    if isinstance(run, str):
        return [run]
    if isinstance(run, list) and all(isinstance(item, str) for item in run):
        return list(run)
    return []


def _load_mise_task_mapping(
    config: dict[str, object],
    *,
    task_name: str,
    path_label: str,
) -> dict[str, object]:
    tasks = config.get("tasks", {})
    if not isinstance(tasks, dict):
        message = f"{path_label} must define [tasks]"
        raise TypeError(message)
    task = tasks.get(task_name)
    if not isinstance(task, dict):
        message = f'{path_label} must define [tasks."{task_name}"]'
        raise TypeError(message)
    return task


def _load_task_depends(task: dict[str, object], *, task_label: str) -> list[str]:
    depends = task.get("depends")
    if isinstance(depends, list) and all(isinstance(item, str) for item in depends):
        return list(depends)
    message = f"{task_label} depends must be a list"
    raise TypeError(message)


def _load_task_run(task: dict[str, object], *, task_label: str) -> str:
    run = task.get("run")
    if isinstance(run, str):
        return run
    message = f"{task_label}.run must be a string"
    raise TypeError(message)


def _get_task_depends(config: dict[str, object], task_name: str) -> list[str]:
    tasks = config.get("tasks", {})
    if not isinstance(tasks, dict):
        return []
    task = tasks.get(task_name)
    if not isinstance(task, dict):
        return []
    depends = task.get("depends")
    if isinstance(depends, list) and all(isinstance(item, str) for item in depends):
        return list(depends)
    return []


def _require_shared_target_dir(
    config: dict[str, object],
    label: str,
) -> str | None:
    if _get_env_value(config, "CARGO_TARGET_DIR") == REQUIRED_SHARED_RUST_TARGET_DIR:
        return None
    return f"{label} must share CARGO_TARGET_DIR=../target/rust"


def _require_task_contains(
    config: dict[str, object],
    task_name: str,
    expected: str,
    message: str,
) -> str | None:
    commands = _get_task_run_commands(config, task_name)
    if any(expected in command for command in commands):
        return None
    return message


def _require_task_excludes(
    config: dict[str, object],
    task_name: str,
    forbidden: str,
    message: str,
) -> str | None:
    commands = _get_task_run_commands(config, task_name)
    if any(forbidden in command for command in commands):
        return message
    return None


def _require_exact_task_run(
    config: dict[str, object],
    task_name: str,
    expected: list[str],
    message: str,
) -> str | None:
    if _get_task_run_commands(config, task_name) == expected:
        return None
    return message


def _require_exact_task_depends(
    config: dict[str, object],
    task_name: str,
    expected: list[str],
    message: str,
) -> str | None:
    if _get_task_depends(config, task_name) == expected:
        return None
    return message


def _require_file_contains(
    path: Path,
    expected_snippets: list[str],
    message: str,
) -> str | None:
    file_text = path.read_text(encoding="utf-8")
    if all(snippet in file_text for snippet in expected_snippets):
        return None
    return message


def _collect_trigger_paths(workflow: dict[str, object], trigger: str) -> set[str]:
    on_block = workflow.get("on", {})
    if not isinstance(on_block, dict):
        return set()
    trigger_block = on_block.get(trigger, {})
    if not isinstance(trigger_block, dict):
        return set()
    paths = trigger_block.get("paths", [])
    if isinstance(paths, list) and all(isinstance(item, str) for item in paths):
        return {item for item in paths if item}
    return set()


def _discover_repo_paths(file_name: str) -> list[str]:
    ignored_parts = {".git", "node_modules", ".venv", "target"}
    discovered: list[str] = []
    for current_root, dirnames, filenames in os.walk(REPO_ROOT):
        dirnames[:] = sorted(name for name in dirnames if name not in ignored_parts)
        if file_name not in filenames:
            continue
        discovered.append(
            Path(current_root, file_name).relative_to(REPO_ROOT).as_posix(),
        )
    return sorted(discovered)


def _matches_any_workflow_pattern(path_text: str, patterns: set[str]) -> bool:
    path = PurePosixPath(path_text)
    for pattern in patterns:
        if not pattern:
            continue
        if "/" not in pattern:
            if path_text == pattern:
                return True
            continue
        if path.match(pattern):
            return True
    return False


def _collect_workflow_step_names(workflow_path: Path) -> set[str]:
    workflow = yaml.safe_load(workflow_path.read_text(encoding="utf-8")) or {}
    if not isinstance(workflow, dict):
        return set()
    jobs = workflow.get("jobs", {})
    if not isinstance(jobs, dict):
        return set()
    step_names: set[str] = set()
    for job in jobs.values():
        if not isinstance(job, dict):
            continue
        steps = job.get("steps", [])
        if not isinstance(steps, list):
            continue
        for step in steps:
            if not isinstance(step, dict):
                continue
            name = step.get("name")
            if isinstance(name, str) and name:
                step_names.add(name)
    return step_names


def _extract_workflow_pin_values(
    workflow_paths: list[Path],
    *,
    uses_fragment: str,
    key: str,
) -> set[str]:
    values: set[str] = set()
    for workflow_path in workflow_paths:
        value = _extract_workflow_pin_value(
            workflow_path=workflow_path,
            uses_fragment=uses_fragment,
            key=key,
        )
        values.add(value)
    return values


def _extract_single_workflow_pin_value(
    workflow_paths: list[Path],
    *,
    uses_fragment: str,
    key: str,
    label: str,
) -> str:
    values = _extract_workflow_pin_values(
        workflow_paths,
        uses_fragment=uses_fragment,
        key=key,
    )
    if len(values) != 1:
        message = f"{label} must be consistent: {sorted(values)}"
        raise AssertionError(message)
    return next(iter(values))


def _extract_workflow_pin_value(
    *,
    workflow_path: Path,
    uses_fragment: str,
    key: str,
) -> str:
    workflow = yaml.safe_load(workflow_path.read_text(encoding="utf-8")) or {}
    jobs = workflow.get("jobs", {})
    if not isinstance(jobs, dict):
        message = f"{workflow_path.name} must define jobs"
        raise TypeError(message)
    for job in jobs.values():
        if not isinstance(job, dict):
            continue
        for step in job.get("steps", []):
            if not isinstance(step, dict):
                continue
            uses = str(step.get("uses", ""))
            if uses_fragment not in uses:
                continue
            with_block = step.get("with", {})
            if isinstance(with_block, dict) and key in with_block:
                return str(with_block[key]).strip().strip('"')
    message = f"{workflow_path.name} must define pinned {key}"
    raise AssertionError(message)


def _find_workflow_step_run(
    workflow_path: Path,
    *,
    job_name: str,
    step_name: str,
) -> str | None:
    workflow = _load_yaml_base_mapping(workflow_path)
    jobs = workflow.get("jobs", {})
    if not isinstance(jobs, dict):
        message = f"{workflow_path.name} must define jobs"
        raise TypeError(message)
    job = jobs.get(job_name)
    if not isinstance(job, dict):
        message = f"{workflow_path.name} must define jobs.{job_name}"
        raise TypeError(message)
    steps = job.get("steps", [])
    if not isinstance(steps, list):
        message = f"{workflow_path.name} jobs.{job_name}.steps must be a list"
        raise TypeError(message)
    for step in steps:
        if not isinstance(step, dict) or step.get("name") != step_name:
            continue
        run = step.get("run")
        return run if isinstance(run, str) else None
    return None


def _collect_build_steps(
    workflow: dict[str, object],
    *,
    seen_workflows: set[Path] | None = None,
) -> list[dict[str, object]]:
    jobs = workflow.get("jobs", {})
    if not isinstance(jobs, dict):
        return []
    if seen_workflows is None:
        seen_workflows = set()
    build_steps: list[dict[str, object]] = []
    for job in jobs.values():
        if not isinstance(job, dict):
            continue
        uses = job.get("uses")
        if isinstance(uses, str) and uses.startswith("./.github/workflows/"):
            nested_workflow_path = (REPO_ROOT / uses.removeprefix("./")).resolve()
            if (
                nested_workflow_path.exists()
                and nested_workflow_path not in seen_workflows
            ):
                seen_workflows.add(nested_workflow_path)
                nested_workflow = _load_workflow(nested_workflow_path)
                build_steps.extend(
                    _collect_build_steps(
                        nested_workflow,
                        seen_workflows=seen_workflows,
                    ),
                )
        steps = job.get("steps", [])
        for step in steps:
            if not isinstance(step, dict):
                continue
            uses = step.get("uses")
            if isinstance(uses, str) and uses.startswith("docker/build-push-action"):
                build_steps.append(step)
    return build_steps


def _find_build_step(
    build_steps: list[dict[str, object]],
    context: str,
) -> dict[str, object] | None:
    for step in build_steps:
        with_block = step.get("with", {})
        if not isinstance(with_block, dict):
            continue
        if with_block.get("context") == context:
            return step
    return None


def _build_contexts(step: dict[str, object]) -> set[str]:
    with_block = step.get("with", {})
    if not isinstance(with_block, dict):
        return set()
    contexts = with_block.get("build-contexts", "")
    if isinstance(contexts, str):
        return {line.strip() for line in contexts.splitlines() if line.strip()}
    return set()


def _require_step(
    label: str,
    step: dict[str, object] | None,
    missing_parts: list[str],
) -> None:
    if step is None:
        missing_parts.append(f"{label} docker/build-push-action step")


def _require_build_contexts(
    label: str,
    step: dict[str, object] | None,
    required: set[str],
    missing_parts: list[str],
) -> None:
    if step is None:
        return
    contexts = _build_contexts(step)
    missing = required.difference(contexts)
    if missing:
        missing_parts.append(
            f"{label} build-contexts missing: " + ", ".join(sorted(missing)),
        )


def _raise_if_missing(missing_parts: list[str]) -> None:
    if missing_parts:
        message = (
            "Docker image workflows are missing required build steps/contexts: "
            + "; ".join(missing_parts)
        )
        raise AssertionError(message)
