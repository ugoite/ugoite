---
name: dependabot-pr-fixer
description: Triage and fix failing Dependabot pull requests in this repo with the smallest safe change, then verify and queue the PR.
---

# Dependabot PR Fixer

Use this skill when a Dependabot PR is failing CI or needs a small follow-up before merge.

## Goal

Make the smallest change that restores CI, keep the fix scoped to the affected workspace, and avoid unrelated refactors.

## Triage

1. Inspect the PR status with `gh pr view <number>` and `gh pr checks <number>`.
2. Read the failing job logs before editing anything.
3. Decide whether the failure is:
   - a stale lockfile
   - a workspace-specific dependency mismatch
   - a real code regression
   - a stale check that only needs a rerun

## Common Fixes

### `uv.lock` needs to be updated

1. Run `uv lock` in the workspace that the failing job uses.
2. If the PR touches `backend`, update `backend/uv.lock`.
3. If the PR touches `ugoite-core`, update `ugoite-core/uv.lock`.
4. Commit only the lockfile diff.
5. Verify with `uv sync --locked`.

### Rust storage failures like `scheme fs is not registered`

1. Update `ugoite-core/src/storage/mod.rs` to build local `fs://` and `memory://` operators explicitly.
2. Prefer `Fs` and `Memory` services instead of `Operator::from_uri` for those local schemes.
3. Add a focused regression test in the same file.
4. Verify with `cargo check -p ugoite-core`.

### Stale checks with no code diff

1. If the branch already has the right code and the PR is only waiting on old results, create an empty commit with `--allow-empty`.
2. Push the branch to retrigger CI.

## Verification

1. Re-run the smallest command that exercises the failing path.
2. Prefer:
   - `uv sync --locked`
   - `cargo check -p ugoite-core`
3. Avoid broad test runs unless the failure is ambiguous.

## Queueing

1. Wait for all required checks to pass.
2. Enqueue the PR using GitHub's merge queue mutation or the repo's normal merge-queue flow.
3. Use the current head OID when queuing so stale revisions are rejected.

## Rules Of Thumb

- Keep changes minimal.
- Never fold unrelated dependency upgrades into the fix.
- Prefer workspace-local lockfile updates over manual edits.
- Add a regression test when the failure is a real code path, not a stale lockfile.
