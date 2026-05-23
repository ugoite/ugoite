---
name: ugoite-orient
description: Use when starting work in the Ugoite repo, choosing the right docs, or deciding which surface and commands apply.
---

# Ugoite Orientation

Use this skill first when the task is ambiguous, cross-surface, or mostly about
figuring out where to work.

## Read first

- `README.md`
- `docs/spec/index.md`
- `docs/spec/testing/ci-cd.md`
- `docs/spec/testing/strategy.md`
- `.github/workflows/`

## What to determine

1. Which surface owns the change: `backend`, `frontend`, `docsite`,
   `ugoite-core`, `ugoite-cli`, `ugoite-minimum`, `docs`, `e2e`, or release.
2. Whether the task is user-facing, spec-facing, or purely operational.
3. Which validation lane matches the change.

## Rules of thumb

- Prefer the smallest surface that can safely absorb the change.
- If docs and code disagree, treat `docs/spec/` and CI as the source of truth.
- For OpenAI model guidance or prompting questions, use the current official
  OpenAI docs rather than a frozen alias or repo-local guess.
- Do not start editing until the owning surface and validation path are clear.

## Output expectation

Give a short routing answer:

- target surface
- primary docs to read
- exact validation command(s) to run next

