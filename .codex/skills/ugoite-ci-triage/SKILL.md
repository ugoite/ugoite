---
name: ugoite-ci-triage
description: Use when a Ugoite CI job fails, an E2E flow times out, or a local/CI process appears to hang.
---

# Ugoite CI Triage

This skill is for diagnosis and the smallest corrective change. It does not
authorize unrelated refactoring or a full repository test run.

## Triage order

1. Read `AGENTS.md`, `docs/spec/testing/ci-cd.md`, and the relevant workflow.
2. Identify the exact run, job, first failing step, and commit.
3. Re-run the smallest failed command or endpoint locally.
4. Compare the local command with the workflow environment and inputs.
5. If it hangs, capture process state, stack/sample, open files/locks, sockets,
   and the last visible request or log line.
6. Classify the cause as implementation, test boundedness, environment, or
   infrastructure before editing.
7. Change only the owning surface, add a focused regression test, and rerun
   the bounded reproducer.

## Hang boundaries

Pay special attention to synchronous calls inside async functions:

- `flock` and filesystem metadata calls;
- DNS and object-storage credential discovery;
- HTTP readiness probes;
- Docker and Playwright startup.

An async timeout cannot fire while the executor thread is blocked in a
synchronous call. Use `spawn_blocking` to keep the executor responsive, but do
not treat it as termination: a stuck blocking closure can continue running.
Use cooperative cancellation when the operation supports it; use a separate
process or a platform-appropriate process-level timeout when the operation
must be forcibly terminated.

## CI and review handoff

Report:

```text
RUN: <url or id>
JOB/STEP: <job and first failing step>
COMMIT: <sha>
CLASSIFICATION: implementation | boundedness | environment | infrastructure
REPRODUCER: <command or endpoint>
FIX: <short description>
VALIDATION: <focused results>
LIMITATIONS: <remaining limitations>
```

Do not hide environment failures inside a code change. Do not run the full
merge gate locally unless the changed surface and evidence justify it.

If a PR is being prepared, keep the public PR body limited to externally safe
facts. Put private environment details and raw diagnostics in the internal
handoff, not the PR description.
