---
name: ugoite-implement
description: Use when implementing, refactoring, or fixing Ugoite code or docs and you need the repo-specific workflow.
---

# Ugoite Implementation

Use this skill when you are making a change rather than just investigating.

## Workflow

1. Read the owning surface docs and relevant spec entries.
2. Make the smallest change that satisfies the requirement.
3. Add or update tests in the same surface.
4. Run the narrowest CI-aligned check first.
5. Expand to the repo-wide check only when the change crosses surfaces.

## Surface map

- `backend`: FastAPI and Python service logic.
- `frontend`: SolidStart UI.
- `docsite`: documentation app and doc rendering checks.
- `ugoite-core`: Rust core plus Python bindings.
- `ugoite-cli`: Rust CLI and release-facing command behavior.
- `ugoite-minimum`: portable Rust core / WASM-oriented logic.
- `e2e`: Playwright flows against running services.
- `docs`: spec, guide, and requirement consistency.

## Change discipline

- Keep behavior changes narrow unless the task explicitly asks for a broader
  migration.
- Prefer repo-native abstractions and existing patterns over introducing new
  ones.
- If a change touches requirements or user-visible behavior, update the
  relevant docs/specs at the same time.
- Do not rename public interfaces, command flags, or file layouts unless the
  task requires it.

## Pull Request Creation

- When the task ends with opening a PR, write the PR body to a file and use
  `scripts/create_pr.py` instead of composing the body inline.
- Keep the PR body aligned with `.github/pull_request_template.md` so the repo's
  PR validation gate stays green on the first attempt.

## OpenAI guidance

- When the task is about AI guidance rather than repo code, follow the current
  official OpenAI docs and model guidance instead of embedding a fixed model id
  in repo instructions.
