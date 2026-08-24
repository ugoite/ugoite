---
name: ugoite-orient
description: Use when starting work in the Ugoite repo, routing a change to the owning surface, or choosing the correct validation lane.
---

# Ugoite Orientation

Use this skill before implementation when the task crosses surfaces, refers to
CI or storage, or the owning code path is not already known.

## Read first

- `AGENTS.md`
- `README.md`
- `docs/spec/index.md`
- `docs/spec/testing/ci-cd.md`
- `docs/spec/testing/strategy.md`
- `mise.toml`
- the relevant workflow under `.github/workflows/`

## Current architecture

- `ugoite-domain`: pure domain types and validation.
- `ugoite-api-client`: transport-neutral remote operation protocol.
- `ugoite-storage`: filesystem/object-store mechanics.
- `ugoite-core`: application behavior.
- `ugoite-iceberg`: storage-backed forms, entries, authorization, search, and derived relations.
- `ugoite-server`, `ugoite-cli`, `ugoite-wasm`: thin adapters.
- `frontend`: SolidStart UI using the portable protocol.
- `docsite`: Astro documentation.
- `e2e`: Deno/Playwright acceptance flows.

Do not route a change to a Python or FastAPI surface unless the repository
contains an explicit current implementation and spec requiring it.

## Routing questions

Determine:

1. Which crate or surface owns the behavior.
2. Whether the change is authoritative, derived, adapter-only, user-facing,
   spec-facing, or operational.
3. Which source of truth and invariant must remain valid.
4. Which root `mise.toml` task or focused crate test is the smallest useful
   validation.
5. Whether the change affects the `quality`, `artifacts`, or `merge_group`
   CI lane.

Only root tasks from `mise.toml` are valid. Use Deno tasks for frontend,
docsite, and E2E work; do not invent package-scoped `mise` task names.

## Worktree preflight

Before editing, record the current branch, worktree status, exact base commit,
and any pre-existing user changes. Never overwrite unrelated changes.

## Output

Give a short routing result:

- owning surface and files to inspect;
- source-of-truth/invariant;
- focused validation command;
- broader CI lane, only if the change requires it.
