"""REQ-OPS-036 regression coverage for contributor workflow documentation."""

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
README = REPO_ROOT / "README.md"
CONTRIBUTING = REPO_ROOT / "CONTRIBUTING.md"


def test_docs_req_ops_036_contributor_workflow_guide_stays_traceable() -> None:
    """REQ-OPS-036: contributor workflow docs stay traceable and CI-aligned."""
    readme_text = README.read_text(encoding="utf-8")
    contributing_text = CONTRIBUTING.read_text(encoding="utf-8")

    details = [
        message
        for condition, message in (
            (
                not CONTRIBUTING.exists(),
                "CONTRIBUTING.md must exist as the canonical contributor guide",
            ),
            (
                "[Contributor Workflow](CONTRIBUTING.md)" not in readme_text,
                "README.md must link to CONTRIBUTING.md from the setup section",
            ),
            (
                "mise run setup" not in contributing_text,
                "CONTRIBUTING.md must lead with the managed setup path",
            ),
            (
                "uvx pre-commit install" not in contributing_text,
                "CONTRIBUTING.md must mention the repo hook bootstrap",
            ),
            (
                ".github/workflows/" not in contributing_text,
                "CONTRIBUTING.md must point contributors at CI workflow sources",
            ),
            (
                "docs/spec/features/" not in contributing_text,
                "CONTRIBUTING.md must explain when feature specs change",
            ),
            (
                "docs/spec/requirements/" not in contributing_text,
                "CONTRIBUTING.md must explain when requirement specs change",
            ),
            (
                "REQ-*" not in contributing_text,
                "CONTRIBUTING.md must describe REQ traceability expectations",
            ),
            (
                "docs/tests/" not in contributing_text,
                "CONTRIBUTING.md must mention docs consistency tests",
            ),
            (
                "docsite/src/lib/navigation.ts" not in contributing_text,
                "CONTRIBUTING.md must mention docsite navigation wiring",
            ),
            (
                "docsite/src/lib/navigation.test.ts" not in contributing_text,
                "CONTRIBUTING.md must mention docsite navigation regression coverage",
            ),
            (
                "mise run test" not in contributing_text,
                "CONTRIBUTING.md must mention the repo-wide validation command",
            ),
            (
                "mise run test:docs" not in contributing_text,
                "CONTRIBUTING.md must mention the docs consistency validation command",
            ),
            (
                "docs, spec, or REQ-traceability changes" not in contributing_text,
                "CONTRIBUTING.md must explain when to run the docs validation command",
            ),
        )
        if condition
    ]

    if details:
        raise AssertionError("; ".join(details))
