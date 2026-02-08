"""Guide validation tests.

REQ-OPS-001: Developer guides must be present with valid bash snippets.
REQ-OPS-002: Docker build CI workflow must be declared.
"""

from __future__ import annotations

import re
import shlex
import textwrap
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
GUIDE_DIR = REPO_ROOT / "docs" / "guide"
WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "docker-build-ci.yml"

CODE_BLOCK_PATTERN = re.compile(
    r"```(?:bash|sh|shell)\s*\n(.*?)\n```",
    re.DOTALL,
)


def _iter_bash_blocks(text: str) -> list[str]:
    return [block.strip() for block in CODE_BLOCK_PATTERN.findall(text)]


def _iter_logical_lines(script: str) -> list[str]:
    logical_lines: list[str] = []
    buffer: list[str] = []
    for raw_line in script.splitlines():
        stripped = raw_line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if stripped.startswith(("$", ">")):
            message = "Shell prompts are not allowed in bash blocks"
            raise AssertionError(message)
        if stripped.endswith("\\"):
            buffer.append(stripped[:-1].rstrip())
            continue
        if buffer:
            buffer.append(stripped)
            logical_lines.append(" ".join(buffer).strip())
            buffer.clear()
        else:
            logical_lines.append(stripped)
    if buffer:
        logical_lines.append(" ".join(buffer).strip())
    return logical_lines


def _validate_shell_line(line: str, source: Path) -> None:
    try:
        shlex.split(line, posix=True)
    except ValueError as exc:
        message = f"Shell parsing failed for {source.relative_to(REPO_ROOT)}: {exc}"
        raise AssertionError(message) from exc


def _bash_syntax_check(script: str, source: Path) -> None:
    normalized = textwrap.dedent(script).strip()
    if not normalized:
        message = f"Empty bash block found in {source.relative_to(REPO_ROOT)}"
        raise AssertionError(message)

    for line in _iter_logical_lines(normalized):
        _validate_shell_line(line, source)


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

    workflow_text = WORKFLOW_PATH.read_text(encoding="utf-8")
    required_markers = [
        "Build backend image",
        "Build frontend image",
        "docker/build-push-action",
    ]
    missing = [marker for marker in required_markers if marker not in workflow_text]
    if missing:
        missing_list = ", ".join(missing)
        message = (
            f"Docker build CI workflow is missing required markers: {missing_list}"
        )
        raise AssertionError(message)
