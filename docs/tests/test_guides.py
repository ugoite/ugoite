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
REQ-OPS-017: Release publish must push GHCR images and document a quick start.
REQ-OPS-018: CLI release binaries and install path must stay documented and wired.
REQ-OPS-019: Mise monorepo config roots must be explicit and complete.
REQ-OPS-021: Frontend 100% coverage must be explicit in CI and root mise test.
"""

from __future__ import annotations

import json
import os
import re
import textwrap
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
UGOITE_CORE_MISE_PATH = REPO_ROOT / "ugoite-core" / "mise.toml"
UGOITE_CLI_MISE_PATH = REPO_ROOT / "ugoite-cli" / "mise.toml"
FRONTEND_MISE_PATH = REPO_ROOT / "frontend" / "mise.toml"
CLI_GUIDE_PATH = GUIDE_DIR / "cli.md"
INSTALL_CLI_SCRIPT_PATH = REPO_ROOT / "scripts" / "install-ugoite-cli.sh"
CONTAINER_QUICKSTART_GUIDE_PATH = GUIDE_DIR / "container-quickstart.md"
DEV_SEED_SCRIPT_PATH = REPO_ROOT / "scripts" / "dev-seed.sh"
ENV_MATRIX_PATH = GUIDE_DIR / "env-matrix.md"
LOCAL_DEV_AUTH_GUIDE_PATH = REPO_ROOT / "docs" / "guide" / "local-dev-auth-login.md"
WAIT_FOR_HTTP_PATH = REPO_ROOT / "scripts" / "wait-for-http.sh"
RUST_TARGET_CLEANUP_PATH = REPO_ROOT / "scripts" / "cleanup-rust-targets.sh"
RELEASE_MANIFEST_PATH = REPO_ROOT / ".github" / ".release-please-manifest.json"
ROOT_PACKAGE_JSON_PATH = REPO_ROOT / "package.json"
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
    "Check root placeholder artifacts",
    "Run yamllint",
    "Run actionlint",
}
REQUIRED_SHARED_RUST_TARGET_DIR = "../target/rust"
REQUIRED_RUST_PRE_COMMIT_HOOKS = {
    "rustfmt",
    "cargo-clippy",
    "cargo-clippy-cli",
    "cargo-llvm-cov-core",
    "cargo-test-cli",
}
REQUIRED_RUST_CI_STEPS = {"Run tests (cli)"}
REQUIRED_RELEASE_CI_PERMISSIONS = {
    "contents": "write",
    "issues": "write",
    "pull-requests": "write",
}
REQUIRED_RELEASE_CI_TOKEN_FRAGMENTS = {
    "secrets.RELEASE_PLEASE_TOKEN != ''",
    "secrets.RELEASE_PLEASE_TOKEN == ''",
    "SKIP_NO_RELEASE_PLEASE_TOKEN",
}
REQUIRED_RELEASE_CI_DOC_FRAGMENTS = {
    "RELEASE_PLEASE_TOKEN",
    "no-op cleanly",
    "bootstrap-sha",
    "0.0.1",
}
REQUIRED_CLI_RELEASE_WORKFLOW_FRAGMENTS = {
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "cargo build --locked --release --bin ugoite --target",
    "gh release upload",
    "ugoite-v${VERSION}-",
    "permissions:",
    "contents: write",
}
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
    "releases/latest",
    "uname -s",
    "uname -m",
    "install -m 0755",
    "sha256sum",
    "shasum -a 256",
}
REQUIRED_CLI_README_FRAGMENTS = {
    "install-ugoite-cli.sh",
    "ugoite --help",
    "UGOITE_VERSION=0.1.0",
}
REQUIRED_CLI_GUIDE_FRAGMENTS = {
    "install-ugoite-cli.sh",
    "ugoite --help",
    "cargo build",
    "cargo run -q -p ugoite-cli -- --help",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
}
REQUIRED_CLI_CICD_FRAGMENTS = {
    ".github/workflows/cli-release-binaries.yml",
    "scripts/install-ugoite-cli.sh",
    "ugoite --help",
}
REQUIRED_RELEASE_PUBLISH_PERMISSIONS = {
    "contents": "write",
    "packages": "write",
}
REQUIRED_RELEASE_PUBLISH_WORKFLOW_FRAGMENTS = {
    "./.github/workflows/docker-images.yml",
    "push: true",
    "version: ${{ inputs.version }}",
    "channel: ${{ inputs.channel }}",
    "target: ${{ inputs.target }}",
}
REQUIRED_DOCKER_IMAGES_REUSABLE_FRAGMENTS = {
    "workflow_call",
    "docker/login-action@v4",
    "registry: ghcr.io",
    "ghcr.io/${{ github.repository }}/backend",
    "ghcr.io/${{ github.repository }}/frontend",
    "$IMAGE:latest",
    "$IMAGE:stable",
    "$IMAGE:$CHANNEL",
}
REQUIRED_RELEASE_QUICKSTART_README_FRAGMENTS = {
    "docker-compose.release.yaml",
    "ghcr.io/ugoite/ugoite/backend",
    "ghcr.io/ugoite/ugoite/frontend",
    "UGOITE_VERSION=0.0.1 docker compose -f docker-compose.release.yaml up -d",
}
REQUIRED_RELEASE_QUICKSTART_GUIDE_FRAGMENTS = {
    "ghcr.io/ugoite/ugoite/backend",
    "ghcr.io/ugoite/ugoite/frontend",
    "UGOITE_VERSION=0.0.1 docker compose -f docker-compose.release.yaml pull",
    "UGOITE_VERSION=0.0.1 docker compose -f docker-compose.release.yaml up -d",
    "latest",
    "stable",
    "alpha",
    "beta",
}
REQUIRED_RELEASE_COMPOSE_FRAGMENTS = {
    "ghcr.io/ugoite/ugoite/backend:${UGOITE_VERSION:?set UGOITE_VERSION}",
    "ghcr.io/ugoite/ugoite/frontend:${UGOITE_VERSION:?set UGOITE_VERSION}",
    "UGOITE_ROOT=/data",
    "BACKEND_URL=http://backend:8000",
}
REQUIRED_RELEASE_QUICKSTART_CICD_FRAGMENTS = {
    "ghcr.io/ugoite/ugoite/backend",
    "ghcr.io/ugoite/ugoite/frontend",
    "docker-compose.release.yaml",
}
REQUIRED_DOCSITE_PRE_COMMIT_HOOKS = {
    "docsite-biome-ci",
    "docsite-format-check",
    "docsite-typecheck",
    "docsite-validation-test",
}
REQUIRED_DOCSITE_CI_STEPS = {
    "Lint",
    "Format check",
    "Typecheck",
    "Validation test (build)",
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
    "UGOITE_DEV_TOTP_CODE",
    "UGOITE_DEV_MANUAL_TOKEN",
    "UGOITE_DEV_MOCK_OAUTH_TOKEN",
    "oathtool",
    "derive a deterministic bearer token",
    "0600",
}
REQUIRED_LOCAL_DEV_AUTH_MODE_README_FRAGMENTS = {
    "UGOITE_DEV_AUTH_MODE=manual-totp",
    "UGOITE_DEV_AUTH_MODE=mock-oauth",
    "Local Dev Auth/Login",
}
REQUIRED_LOCAL_DEV_AUTH_MODE_ENV_MATRIX_VARS = {
    "| UGOITE_DEV_AUTH_MODE |",
    "| UGOITE_DEV_TOTP_CODE |",
    "| UGOITE_DEV_MANUAL_TOKEN |",
    "| UGOITE_DEV_MOCK_OAUTH_TOKEN |",
}
REQUIRED_LOCAL_DEV_AUTH_SCRIPT_FRAGMENTS = {
    'AUTH_MODE="${UGOITE_DEV_AUTH_MODE:-automatic}"',
    "UGOITE_DEV_TOTP_CODE",
    "UGOITE_DEV_MANUAL_TOKEN",
    "UGOITE_DEV_MOCK_OAUTH_TOKEN",
    "path.chmod(0o600)",
    "require_working_oathtool",
    'announce_mode "automatic"',
    'announce_mode "manual-totp"',
    'announce_mode "mock-oauth"',
    "manual-totp mode requires one of:",
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

    bun_versions = _extract_workflow_pin_values(
        [DOCSITE_CI_WORKFLOW_PATH, FRONTEND_CI_WORKFLOW_PATH],
        uses_fragment="setup-bun",
        key="bun-version",
    )
    rust_versions = _extract_workflow_pin_values(
        [RUST_CI_WORKFLOW_PATH],
        uses_fragment="rust-toolchain",
        key="toolchain",
    )
    python_versions = _extract_workflow_pin_values(
        [SCANCODE_WORKFLOW_PATH],
        uses_fragment="scancode-action",
        key="python-version",
    )

    if len(bun_versions) != 1:
        message = f"CI bun-version pins must be consistent: {sorted(bun_versions)}"
        raise AssertionError(message)
    if len(rust_versions) != 1:
        message = f"CI rust toolchain pins must be consistent: {sorted(rust_versions)}"
        raise AssertionError(message)
    if len(python_versions) != 1:
        message = (
            f"CI python-version pins must be consistent: {sorted(python_versions)}"
        )
        raise AssertionError(message)

    bun_version = next(iter(bun_versions))
    rust_version = next(iter(rust_versions))
    python_version = next(iter(python_versions))

    if str(tools.get("bun")) != bun_version:
        message = "mise.toml [tools].bun must match CI bun-version"
        raise AssertionError(message)
    if str(tools.get("rust")) != rust_version:
        message = "mise.toml [tools].rust must match CI rust toolchain"
        raise AssertionError(message)
    if str(tools.get("python")) != python_version:
        message = "mise.toml [tools].python must match CI python-version"
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

    if missing_hooks or missing_steps or leaked_steps:
        details: list[str] = []
        if missing_hooks:
            details.append("pre-commit missing hooks: " + ", ".join(missing_hooks))
        if missing_steps:
            details.append(
                "yaml-workflow-ci missing steps: " + ", ".join(missing_steps),
            )
        if leaked_steps:
            details.append("python-ci should not include: " + ", ".join(leaked_steps))
        raise AssertionError("; ".join(details))


def test_docs_req_ops_006_rust_precommit_parity() -> None:
    """REQ-OPS-006: Rust pre-commit checks must include CLI test parity with CI."""
    pre_commit = _load_pre_commit_config()
    configured_hooks = _collect_pre_commit_hook_ids(pre_commit)
    missing_hooks = sorted(REQUIRED_RUST_PRE_COMMIT_HOOKS.difference(configured_hooks))

    hook_entries = _collect_pre_commit_hooks(pre_commit)
    clippy_cli_entry = ""
    cargo_clippy_cli = hook_entries.get("cargo-clippy-cli")
    if isinstance(cargo_clippy_cli, dict):
        clippy_cli_entry = str(cargo_clippy_cli.get("entry", ""))

    missing_parts: list[str] = []
    if missing_hooks:
        missing_parts.append("pre-commit missing hooks: " + ", ".join(missing_hooks))
    if "--no-default-features" not in clippy_cli_entry:
        missing_parts.append("cargo-clippy-cli must pass --no-default-features")

    rust_ci_steps = _collect_workflow_step_names(RUST_CI_WORKFLOW_PATH)
    missing_ci_steps = sorted(REQUIRED_RUST_CI_STEPS.difference(rust_ci_steps))
    if missing_ci_steps:
        missing_parts.append("rust-ci missing steps: " + ", ".join(missing_ci_steps))

    if missing_parts:
        raise AssertionError("; ".join(missing_parts))


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
                "cargo clean -p ugoite-core",
                "ugoite-core build task must clean package-local Rust artifacts",
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
                "cargo clean -p ugoite-cli",
                "ugoite-cli test task must clean package-local Rust artifacts",
            ),
            _require_exact_task_run(
                root_mise,
                "cleanup:rust-targets",
                ["bash scripts/cleanup-rust-targets.sh"],
                "root mise must expose cleanup:rust-targets",
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
                ["mise run cleanup:rust-targets"],
                "README must document cleanup:rust-targets",
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
    """REQ-OPS-015: Local dev auth docs must cover explicit manual modes."""
    guide_text = LOCAL_DEV_AUTH_GUIDE_PATH.read_text(encoding="utf-8")
    readme_text = README_PATH.read_text(encoding="utf-8")
    env_matrix_text = ENV_MATRIX_PATH.read_text(encoding="utf-8")

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

    missing_env_matrix = sorted(
        fragment
        for fragment in REQUIRED_LOCAL_DEV_AUTH_MODE_ENV_MATRIX_VARS
        if fragment not in env_matrix_text
    )
    if missing_env_matrix:
        details.append(
            "env-matrix.md missing manual auth vars: " + ", ".join(missing_env_matrix),
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


def test_docs_req_ops_017_release_publish_pushes_ghcr_and_docs_quickstart() -> None:
    """REQ-OPS-017: Release Publish must push GHCR images and document a quick start."""
    details = _collect_release_publish_ghcr_details()
    if details:
        raise AssertionError("; ".join(details))


def test_docs_req_ops_018_cli_release_binaries_and_install_script() -> None:
    """REQ-OPS-018: CLI release binaries and install path stay aligned."""
    details = _collect_cli_release_install_details()
    if details:
        raise AssertionError("; ".join(details))


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


def test_docs_req_ops_021_frontend_coverage_gate_is_explicit() -> None:
    """REQ-OPS-021: Frontend 100% coverage must be explicit in CI and root mise test."""
    root_mise = tomllib.loads(MISE_PATH.read_text(encoding="utf-8"))
    root_test = root_mise.get("tasks", {}).get("test")
    if not isinstance(root_test, dict):
        raise TypeError("root mise.toml must define tasks.test")

    depends = root_test.get("depends")
    if not isinstance(depends, list):
        raise TypeError("root mise.toml tasks.test depends must be a list")

    if "//frontend:test:coverage" not in depends:
        raise AssertionError(
            "root mise.toml tasks.test must depend on //frontend:test:coverage",
        )

    frontend_mise = tomllib.loads(FRONTEND_MISE_PATH.read_text(encoding="utf-8"))
    frontend_tasks = frontend_mise.get("tasks", {})
    if not isinstance(frontend_tasks, dict):
        raise TypeError("frontend/mise.toml must define [tasks]")

    coverage_task = frontend_tasks.get("test:coverage")
    if not isinstance(coverage_task, dict):
        raise AssertionError('frontend/mise.toml must define [tasks."test:coverage"]')

    coverage_run = coverage_task.get("run")
    if not isinstance(coverage_run, str):
        raise TypeError('frontend/mise.toml [tasks."test:coverage"].run must be a string')
    if "node ./node_modules/vitest/vitest.mjs run --coverage" not in coverage_run:
        raise AssertionError(
            'frontend/mise.toml [tasks."test:coverage"] must run node vitest coverage',
        )

    workflow = _load_yaml_base_mapping(FRONTEND_CI_WORKFLOW_PATH)
    jobs = workflow.get("jobs", {})
    if not isinstance(jobs, dict):
        raise TypeError("frontend-ci.yml must define jobs")
    ci_job = jobs.get("ci")
    if not isinstance(ci_job, dict):
        raise TypeError("frontend-ci.yml must define jobs.ci")
    steps = ci_job.get("steps", [])
    if not isinstance(steps, list):
        raise TypeError("frontend-ci.yml jobs.ci.steps must be a list")

    coverage_step_run: str | None = None
    for step in steps:
        if not isinstance(step, dict):
            continue
        if step.get("name") == "Run Vitest with 100% coverage gate":
            run = step.get("run")
            if isinstance(run, str):
                coverage_step_run = run
            break

    if coverage_step_run is None:
        raise AssertionError("frontend-ci.yml must define the frontend coverage gate step")
    if "node ./node_modules/vitest/vitest.mjs run --coverage" not in coverage_step_run:
        raise AssertionError("frontend-ci.yml coverage gate must run node vitest coverage")

    guide_text = CI_CD_SPEC_PATH.read_text(encoding="utf-8")
    missing_doc_fragments = sorted(
        fragment
        for fragment in REQUIRED_FRONTEND_COVERAGE_DOC_FRAGMENTS
        if fragment not in guide_text
    )
    if missing_doc_fragments:
        raise AssertionError(
            "ci-cd guide missing frontend coverage fragments: "
            + ", ".join(missing_doc_fragments),
        )


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
    package_data = _load_json_mapping(ROOT_PACKAGE_JSON_PATH)
    bootstrap_sha = str(release_config.get("bootstrap-sha", "")).strip()
    manifest_version = str(manifest.get(".", ""))
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
            f"package.json version must start at 0.0.1 (got {package_version!r})",
        ),
        (
            bool(missing_doc_fragments),
            "ci-cd guide missing fragments: " + ", ".join(missing_doc_fragments),
        ),
    )
    return [message for condition, message in detail_candidates if condition]


def _collect_cli_release_install_details() -> list[str]:
    workflow_text = RELEASE_PUBLISH_WORKFLOW_PATH.read_text(encoding="utf-8")
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
        CLI_RELEASE_WORKFLOW_PATH.read_text(encoding="utf-8"),
        REQUIRED_CLI_RELEASE_WORKFLOW_FRAGMENTS,
    )
    missing_install_script = _missing_required_fragments(
        INSTALL_CLI_SCRIPT_PATH.read_text(encoding="utf-8"),
        REQUIRED_INSTALL_CLI_SCRIPT_FRAGMENTS,
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
            bool(missing_install_script),
            "install-ugoite-cli.sh missing fragments: "
            + ", ".join(missing_install_script),
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


def _collect_release_publish_ghcr_details() -> list[str]:
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

    publish_images_uses, release_needs = _collect_release_publish_jobs(workflow)

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
            "publish-images" not in release_needs,
            "release-publish publish-release job must depend on publish-images",
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


def _collect_release_publish_jobs(
    workflow: dict[object, object],
) -> tuple[str, list[object]]:
    jobs = workflow.get("jobs", {})
    if not isinstance(jobs, dict):
        message = "release-publish.yml must define jobs"
        raise TypeError(message)

    publish_images_job = jobs.get("publish-images")
    publish_release_job = jobs.get("publish-release")
    if not isinstance(publish_images_job, dict):
        message = "release-publish.yml must define jobs.publish-images"
        raise TypeError(message)
    if not isinstance(publish_release_job, dict):
        message = "release-publish.yml must define jobs.publish-release"
        raise TypeError(message)

    publish_images_uses = str(publish_images_job.get("uses", "")).strip()
    release_needs = publish_release_job.get("needs", [])
    if isinstance(release_needs, str):
        release_needs = [release_needs]
    if not isinstance(release_needs, list):
        message = (
            "release-publish.yml jobs.publish-release.needs must be a string or list"
        )
        raise TypeError(message)
    return publish_images_uses, release_needs


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


def _require_exact_task_run(
    config: dict[str, object],
    task_name: str,
    expected: list[str],
    message: str,
) -> str | None:
    if _get_task_run_commands(config, task_name) == expected:
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
