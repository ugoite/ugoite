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
4. If the PR was green but merge queue rejected it, assume the base moved and rebase onto `origin/main` before changing anything else.

## Common Fixes

### `uv.lock` needs to be updated

1. Run `uv lock` in the workspace that the failing job uses.
2. If the PR touches `backend`, update `backend/uv.lock`.
3. If the PR touches `ugoite-core`, update `ugoite-core/uv.lock`.
4. Commit only the lockfile diff.
5. Verify with `uv sync --locked`.

### Bun lockfiles need to be refreshed

1. Run `bun install` in the affected workspace, usually `docsite/` or `frontend/`.
2. Keep the `package.json` and `bun.lock` pair in sync.
3. Verify with `bun install --frozen-lockfile`.

### Backend `uv.lock` keeps snapping back

1. Check whether `backend` is resolving through `ugoite-core/pyproject.toml` or another sibling workspace pin.
2. If the resolved version is intentionally lower, keep the normalized `backend/uv.lock` and do not force the Dependabot version into the lockfile.
3. If you actually need the newer version across the workspace, raise the source requirement in `ugoite-core/pyproject.toml`, then regenerate both `ugoite-core/uv.lock` and `backend/uv.lock`.
4. Verify with `cd backend && uv sync --locked`.

### Backend Dockerfile pinning failures

1. Keep the `COPY --from=ghcr.io/astral-sh/uv:<version> /uv /uvx /bin/` line pinned to an exact version tag.
2. Keep Python base images pinned with an exact version tag and sha256 digest.
3. Verify with `uv run --with pytest --with pyyaml --with bashlex pytest -W error docs/tests/test_guides.py::test_docs_req_ops_002_container_external_refs_are_pinned -v`.

### Devcontainer smoke hits a release API limit

1. If `devcontainers/ci` fails while installing a feature with `HTTP Error 403: rate limit exceeded`, rerun the workflow once before editing files.
2. Treat the failure as flaky unless the same trace repeats after a rerun.
3. Only change `.devcontainer/devcontainer.json` or feature pins if the rerun reproduces the same install failure.

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
2. Enqueue the PR with `gh pr merge <number>` or the repo's normal merge-queue flow once the checks are green.
3. Use `--match-head-commit <sha>` when you want GitHub to reject stale revisions instead of queueing an old head.
4. If the branch was rebased to fix queue drift, push with `--force-with-lease` and then re-run the queue step.

## Rules Of Thumb

- Keep changes minimal.
- Never fold unrelated dependency upgrades into the fix.
- Prefer workspace-local lockfile updates over manual edits.
- Add a regression test when the failure is a real code path, not a stale lockfile.
