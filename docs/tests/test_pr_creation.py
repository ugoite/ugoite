"""REQ-OPS-042 regression coverage for script-driven PR creation."""

from __future__ import annotations

import importlib.util
from pathlib import Path

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
