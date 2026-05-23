"""Create a GitHub pull request from a validated body file."""

from __future__ import annotations

import argparse
import re
import subprocess
from pathlib import Path
from shutil import which

REQUIRED_SECTIONS = (
    "## Summary",
    "## Related Issue (required)",
    "## Testing",
)
SUMMARY_PLACEHOLDER = "-"
ISSUE_LINK_PATTERN = re.compile(
    r"(?:^|\n)\s*(?:close:\s*#\d+|closes\s+#\d+)\s*(?:\n|$)",
    re.IGNORECASE,
)
SECTION_PATTERN_TEMPLATE = r"##\s*{title}\s*\n+([\s\S]*?)(?:\n##\s|$)"


def parse_args() -> argparse.Namespace:
    """Parse command-line arguments."""
    parser = argparse.ArgumentParser(
        description="Validate a pull request body file and create the PR with gh.",
    )
    parser.add_argument("--title", required=True, help="Pull request title")
    parser.add_argument(
        "--body-file",
        required=True,
        type=Path,
        help="Path to a markdown file that contains the pull request body",
    )
    parser.add_argument(
        "--base",
        default="main",
        help="Base branch to target when creating the pull request",
    )
    parser.add_argument(
        "--draft",
        action="store_true",
        help="Create the pull request as a draft",
    )
    return parser.parse_args()


def extract_body_section(body: str, title: str) -> str:
    """Return the trimmed text inside a markdown section."""
    pattern = re.compile(
        SECTION_PATTERN_TEMPLATE.format(title=re.escape(title)),
        re.IGNORECASE,
    )
    match = pattern.search(body)
    return match.group(1).strip() if match else ""


def validate_pr_body(body: str) -> list[str]:
    """Return validation errors for a pull request body."""
    lowered = body.lower()
    errors = [
        f"Missing required section: {section}"
        for section in REQUIRED_SECTIONS
        if section.lower() not in lowered
    ]

    summary_text = extract_body_section(body, "Summary")
    if (
        not summary_text
        or summary_text == SUMMARY_PLACEHOLDER
        or re.fullmatch(
            r"-\s*",
            summary_text,
        )
    ):
        errors.append(
            "Summary section must be filled in and cannot remain the '-' placeholder.",
        )

    if not ISSUE_LINK_PATTERN.search(body):
        errors.append(
            "Related Issue must include either `close: #123` or `closes #123`.",
        )

    testing_text = extract_body_section(body, "Testing")
    if not re.search(r"(?m)^- \[(?: |x|X)\]", testing_text):
        errors.append("Testing section must include at least one checklist item.")

    return errors


def build_gh_command(
    *,
    title: str,
    body_file: Path,
    base: str,
    draft: bool,
) -> list[str]:
    """Build the safe gh command for PR creation."""
    command = [
        "gh",
        "pr",
        "create",
        "--base",
        base,
        "--title",
        title,
        "--body-file",
        str(body_file),
    ]
    if draft:
        command.insert(3, "--draft")
    return command


def ensure_clean_worktree() -> None:
    """Reject PR creation if the worktree still has unstaged or uncommitted files."""
    git_status_command = ["git", "status", "--porcelain=v1"]
    completed = subprocess.run(
        git_status_command,
        capture_output=True,
        check=True,
        text=True,
    )
    if completed.stdout.strip():
        message = (
            "Working tree must be clean before creating a PR. Commit or stash "
            "changes first."
        )
        raise SystemExit(message)


def main() -> int:
    """Validate the PR body file and create the PR."""
    args = parse_args()
    if not args.body_file.exists():
        message = f"PR body file not found: {args.body_file}"
        raise SystemExit(message)
    if which("gh") is None:
        message = "Required command not found: gh"
        raise SystemExit(message)

    body = args.body_file.read_text(encoding="utf-8")
    errors = validate_pr_body(body)
    if errors:
        raise SystemExit("\n".join(errors))

    ensure_clean_worktree()
    subprocess.run(
        build_gh_command(
            title=args.title,
            body_file=args.body_file,
            base=args.base,
            draft=args.draft,
        ),
        check=True,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
