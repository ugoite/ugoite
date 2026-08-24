---
name: codex-pr-safety
description: Use when creating or updating pull requests from Codex so the PR body is validated from a file before GitHub submission.
---

# Codex PR Safety

Use this skill when you are about to open a pull request from the current branch.

## Read first

- `.github/pull_request_template.md`
- `tools/create_pr.ts`
- `.github/workflows/pr-require-close-issue.yml`

## Workflow

1. Write the pull request body into a markdown file instead of passing it inline on the command line.
2. Keep the body aligned with the repository PR template sections and issue-closing format.
3. Create the PR with `deno run -A tools/create_pr.ts --title "..." --body-file /path/to/body.md [--draft]`.
4. If validation fails, fix the body file and rerun the script.
5. Only use the PR link after the script succeeds.

## Rules of Thumb

- Never pass multiline PR body text directly to `gh pr create`.
- Prefer a body file even when the text is short, so quoting never mutates the content.
- Keep the summary concrete, the related issue explicit, and the testing section checked.

## Public-content check

The PR body is public. Before submission, remove private conversations, local
filesystem paths, credentials, internal environment details, reviewer prompts,
token usage, and private implementation history. Keep only the externally safe
problem statement, behavior/design change, related issue, and validation
evidence.
