"""REQ-OPS-042 regression coverage for script-driven PR creation."""

from __future__ import annotations

import importlib.util
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]
CREATE_PR_PATH = REPO_ROOT / "scripts" / "create_pr.py"


def _load_create_pr_module() -> object:
    spec = importlib.util.spec_from_file_location("create_pr", CREATE_PR_PATH)
    if spec is None or spec.loader is None:
        message = f"Could not load helper module from {CREATE_PR_PATH}"
        raise RuntimeError(message)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_docs_req_ops_042_pr_helper_validates_template_compliant_body() -> None:
    """REQ-OPS-042: PR helper must accept template-compliant body files."""
    create_pr = _load_create_pr_module()
    body = (
        "## Summary\n\n"
        "Keep the PR body on a file-backed path.\n\n"
        "## Related Issue (required)\n\n"
        "close: #1689\n\n"
        "## Testing\n\n"
        "- [x] `mise run test`\n"
    )

    errors = create_pr.validate_pr_body(body)

    assert errors == []


def test_docs_req_ops_042_pr_helper_requires_file_transport() -> None:
    """REQ-OPS-042: PR helper must build a body-file gh command."""
    create_pr = _load_create_pr_module()
    body_file = REPO_ROOT / "tmp" / "pr-body.md"

    command = create_pr.build_gh_command(
        title="docs: harden PR creation",
        body_file=body_file,
        base="main",
        draft=True,
    )

    assert command == [
        "gh",
        "pr",
        "create",
        "--draft",
        "--base",
        "main",
        "--title",
        "docs: harden PR creation",
        "--body-file",
        str(body_file),
    ]


def test_docs_req_ops_042_pr_helper_rejects_untracked_files() -> None:
    """REQ-OPS-042: PR helper must treat untracked files as a dirty worktree."""
    create_pr = _load_create_pr_module()
    completed = MagicMock(stdout="?? scratch.txt\n")

    with (
        patch.object(create_pr.subprocess, "run", return_value=completed) as run,
        pytest.raises(SystemExit, match="Working tree must be clean"),
    ):
        create_pr.ensure_clean_worktree()

    run.assert_called_once_with(
        ["git", "status", "--porcelain=v1"],
        capture_output=True,
        check=True,
        text=True,
    )
