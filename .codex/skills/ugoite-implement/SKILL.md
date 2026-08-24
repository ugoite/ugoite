---
name: ugoite-implement
description: Use when implementing, refactoring, or fixing Ugoite code, docs, or repository workflow guidance.
---

# Ugoite Implementation

Use the smallest workflow that satisfies the requested delivery unit. Keep
authoritative data portable and preserve the architecture in `AGENTS.md`.

## Workflow

1. Read the owning surface docs and relevant spec entries.
2. Confirm worktree, branch, exact base, and pre-existing changes.
3. If a failure is reported, reproduce the same command, endpoint, or CI task
   before broad exploration.
4. Define the smallest coherent delivery unit. Split independent invariants
   into separate issues/PRs when that makes them independently reviewable;
   keep coupled changes together when an intermediate state would not be a
   valid product state.
5. Make the smallest change that satisfies the requirement.
6. Add or update tests in the owning surface.
7. Run the narrowest CI-aligned validation first. Expand only when the changed
   surface or merge gate requires it.
8. Review the result, publish the branch, and use the repository PR/CI flow.

## Diagnosis and implementation discipline

- Follow the real call path from the observed failure to its owning layer.
- Treat synchronous filesystem locks, blocking network calls, DNS, Docker, and
  Playwright readiness as possible hang boundaries.
- Do not rely on `tokio::time::timeout` to protect code that can block the
  executor thread in synchronous I/O. `spawn_blocking` isolates executor
  starvation but does not terminate a stuck closure; use it together with
  cooperative cancellation where available, and use a child process or a
  platform-appropriate process-level timeout when termination is required.
- Prefer repo-native abstractions and existing patterns.
- If requirements or user-visible behavior change, update the relevant spec or
  docs in the same delivery unit.
- Do not rename public interfaces, flags, or file layouts unless required.

## Review convergence

The primary agent owns review orchestration. A reviewer must report the commit
it actually reviewed using this compact record:

```text
REVIEW_BASE: <commit>
REVIEWED_FROM: <previous REVIEWED_HEAD, or none for the first review>
REVIEWED_HEAD: <commit>
REVIEW_SCOPE: <scope>
REVIEWED_INVARIANTS: <invariants checked>
REVIEWED_CHECKS: <checks and CI evidence considered>
VERDICT: APPROVE | CHANGE_REQUEST
CARRIED_BLOCKERS: <none or each prior blocker with RESOLVED | STILL_OPEN>
NEW_BLOCKERS: <none or complete list>
FOLLOW_UPS: <issue links or none>
EVIDENCE: <focused tests/checks>
LIMITATIONS: <environment limitations>
```

The first review covers the defined cumulative scope. Later reviews are
delta-first from `REVIEWED_HEAD`, then verify the cumulative invariants touched
by the new delta. The primary agent keeps a review ledger containing every
review record and passes the previous record to the next reviewer; do not rely
on reviewer memory. Every later reviewer must enumerate all carried blockers
and mark each one `RESOLVED` or `STILL_OPEN`. An omitted or still-open carried
blocker requires `CHANGE_REQUEST`. Do not re-report unchanged findings or
re-run completed checks without new evidence. `CHANGE_REQUEST` is reserved for
a problem that makes the merge unsafe or impossible; other findings become
follow-up issues.

## Public PR content

The PR body is a public artifact. Include only the externally safe problem
statement, behavior change, design impact, related issue, and validation
results. Do not include private conversations, local filesystem paths,
credentials, internal environment details, reviewer prompts, token usage, or
private implementation history.

When opening a PR, use `codex-pr-safety` and the repository PR template.
