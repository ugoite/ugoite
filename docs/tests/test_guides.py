"""Guide validation tests.

REQ-OPS-001: Developer guides must be present with valid bash snippets.
REQ-OPS-002: Docker build CI workflow must be declared.
"""

from __future__ import annotations

import re
import textwrap
from pathlib import Path

import bashlex
import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
GUIDE_DIR = REPO_ROOT / "docs" / "guide"
WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "docker-build-ci.yml"

CODE_BLOCK_PATTERN = re.compile(
    r"```(?:bash|sh|shell)\s*\n(.*?)\n```",
    re.DOTALL,
)


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


def _load_workflow() -> dict[str, object]:
    workflow_text = WORKFLOW_PATH.read_text(encoding="utf-8")
    workflow = yaml.safe_load(workflow_text)
    if isinstance(workflow, dict):
        return workflow
    return {}


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
