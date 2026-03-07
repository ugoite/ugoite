"""Guide validation tests.

REQ-OPS-001: Developer guides must be present with valid bash snippets.
REQ-OPS-002: Docker build CI workflow must be declared.
REQ-OPS-005: YAML/workflow lint gates must be enforced in pre-commit and CI.
REQ-OPS-007: Docsite quality parity must be enforced in pre-commit and CI.
REQ-OPS-009: Release automation bootstrap and PR permissions must be documented.
"""

from __future__ import annotations

import json
import re
import textwrap
from pathlib import Path

import bashlex
import tomllib
import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
GUIDE_DIR = REPO_ROOT / "docs" / "guide"
WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "docker-build-ci.yml"
DOCSITE_CI_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "docsite-ci.yml"
FRONTEND_CI_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "frontend-ci.yml"
PYTHON_CI_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "python-ci.yml"
YAML_WORKFLOW_CI_WORKFLOW_PATH = (
    REPO_ROOT / ".github" / "workflows" / "yaml-workflow-ci.yml"
)
RELEASE_CI_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "release-ci.yml"
RUST_CI_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "rust-ci.yml"
SCANCODE_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "scancode.yml"
PRE_COMMIT_CONFIG_PATH = REPO_ROOT / ".pre-commit-config.yaml"
README_PATH = REPO_ROOT / "README.md"
MISE_PATH = REPO_ROOT / "mise.toml"
ENV_MATRIX_PATH = GUIDE_DIR / "env-matrix.md"
RELEASE_MANIFEST_PATH = REPO_ROOT / ".github" / ".release-please-manifest.json"
ROOT_PACKAGE_JSON_PATH = REPO_ROOT / "package.json"
CI_CD_SPEC_PATH = REPO_ROOT / "docs" / "spec" / "testing" / "ci-cd.md"
COLUMN_COUNT_THRESHOLD = 2
REQUIRED_PRE_COMMIT_HOOKS = {"root-artifact-hygiene", "yamllint", "actionlint"}
REQUIRED_YAML_WORKFLOW_CI_STEPS = {
    "Check root placeholder artifacts",
    "Run yamllint",
    "Run actionlint",
}
REQUIRED_RELEASE_CI_PERMISSIONS = {
    "contents": "write",
    "issues": "write",
    "pull-requests": "write",
}
REQUIRED_RELEASE_CI_TOKEN_FRAGMENTS = {
    "secrets.RELEASE_PLEASE_TOKEN",
    "secrets.GITHUB_TOKEN",
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
    if not WORKFLOW_PATH.exists():
        message = f"Missing workflow file: {WORKFLOW_PATH.relative_to(REPO_ROOT)}"
        raise AssertionError(message)
    workflow = _load_workflow()
    build_steps = _collect_build_steps(workflow)
    backend_step = _find_build_step(build_steps, "./backend")
    frontend_step = _find_build_step(build_steps, "./frontend")

    missing_parts: list[str] = []
    _require_step("backend", backend_step, missing_parts)
    _require_step("frontend", frontend_step, missing_parts)
    _require_build_contexts(
        "backend",
        backend_step,
        {"core=./ugoite-core", "module=./ugoite-cli"},
        missing_parts,
    )
    _require_build_contexts(
        "frontend",
        frontend_step,
        {"shared=./shared"},
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


def test_docs_req_ops_009_release_ci_bootstrap_and_permissions_declared() -> None:
    """REQ-OPS-009: Release CI bootstrap metadata and PR permission docs must stay aligned."""
    workflow = yaml.safe_load(RELEASE_CI_WORKFLOW_PATH.read_text(encoding="utf-8")) or {}
    if not isinstance(workflow, dict):
        message = "release-ci.yml must be a YAML mapping"
        raise TypeError(message)

    workflow_text = RELEASE_CI_WORKFLOW_PATH.read_text(encoding="utf-8")

    permissions = workflow.get("permissions")
    if not isinstance(permissions, dict):
        raise AssertionError("release-ci.yml must define top-level permissions")

    missing_permissions = [
        f"{name}={expected}"
        for name, expected in REQUIRED_RELEASE_CI_PERMISSIONS.items()
        if str(permissions.get(name)) != expected
    ]

    release_ci_steps = _collect_workflow_step_names(RELEASE_CI_WORKFLOW_PATH)
    missing_steps = sorted(
        {"Print release auth path", "Run release-please"}.difference(release_ci_steps),
    )
    missing_token_fragments = sorted(
        fragment
        for fragment in REQUIRED_RELEASE_CI_TOKEN_FRAGMENTS
        if fragment not in workflow_text
    )

    manifest = json.loads(RELEASE_MANIFEST_PATH.read_text(encoding="utf-8"))
    if not isinstance(manifest, dict):
        message = ".release-please-manifest.json must be a JSON mapping"
        raise TypeError(message)

    package_data = json.loads(ROOT_PACKAGE_JSON_PATH.read_text(encoding="utf-8"))
    if not isinstance(package_data, dict):
        message = "package.json must be a JSON mapping"
        raise TypeError(message)

    manifest_version = str(manifest.get(".", ""))
    package_version = str(package_data.get("version", ""))

    guide_text = CI_CD_SPEC_PATH.read_text(encoding="utf-8")
    required_doc_fragments = {
        "Allow GitHub Actions to create and approve pull requests",
        "RELEASE_PLEASE_TOKEN",
        "0.0.1",
    }
    missing_doc_fragments = sorted(
        fragment for fragment in required_doc_fragments if fragment not in guide_text
    )

    details: list[str] = []
    if missing_permissions:
        details.append("release-ci permissions mismatch: " + ", ".join(missing_permissions))
    if missing_steps:
        details.append("release-ci missing steps: " + ", ".join(missing_steps))
    if missing_token_fragments:
        details.append(
            "release-ci missing token fragments: " + ", ".join(missing_token_fragments),
        )
    if manifest_version != "0.0.1":
        details.append(f"release manifest must start at 0.0.1 (got {manifest_version!r})")
    if package_version != "0.0.1":
        details.append(f"package.json version must start at 0.0.1 (got {package_version!r})")
    if missing_doc_fragments:
        details.append("ci-cd guide missing fragments: " + ", ".join(missing_doc_fragments))
    if details:
        raise AssertionError("; ".join(details))


def _load_workflow() -> dict[str, object]:
    workflow_text = WORKFLOW_PATH.read_text(encoding="utf-8")
    workflow = yaml.safe_load(workflow_text)
    if isinstance(workflow, dict):
        return workflow
    return {}


def _load_pre_commit_config() -> dict[str, object]:
    config_text = PRE_COMMIT_CONFIG_PATH.read_text(encoding="utf-8")
    config = yaml.safe_load(config_text)
    if isinstance(config, dict):
        return config
    message = ".pre-commit-config.yaml must be a YAML mapping"
    raise TypeError(message)


def _collect_pre_commit_hook_ids(config: dict[str, object]) -> set[str]:
    repos = config.get("repos", [])
    if not isinstance(repos, list):
        return set()
    hook_ids: set[str] = set()
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
                hook_ids.add(hook_id)
    return hook_ids


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


def _collect_build_steps(workflow: dict[str, object]) -> list[dict[str, object]]:
    jobs = workflow.get("jobs", {})
    if not isinstance(jobs, dict):
        return []
    build_steps: list[dict[str, object]] = []
    for job in jobs.values():
        steps = job.get("steps", []) if isinstance(job, dict) else []
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
            "Docker build CI workflow is missing required build steps/contexts: "
            + "; ".join(missing_parts)
        )
        raise AssertionError(message)
