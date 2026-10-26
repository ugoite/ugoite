---
title: Composition / SQL Workspace acceptance checkpoint — 2026-10-26
---

**Overall: 未達 (not complete). `coverage_complete`: false.** The original
[30-goal ledger](composition-acceptance-2026-10-25.md#goal-ledger) and
[acceptance criteria](composition-acceptance-2026-10-23.md) remain unchanged.
This checkpoint adds bounded evidence without narrowing the goals. All 29
previously unverified verdicts and the unmet S-02 verdict remain unchanged.

The shared Filter editor now shows existing comparisons as readable summary rows
and opens one comparison editor or add composer at a time. Nested all/any
groups, typed values and arguments, removal, and Undo/Redo use guarded Rust SQL
Document transactions. SQL, argument declarations, and Composition scalar
bindings commit together. Unsupported query scopes retain explicit advanced
editing. The previous add-only Filter implementation has been removed.

Pending edits are invalidated when structural edits can change parser node IDs;
a rejected transaction retains its input. History and removal restore focus
inside the editor. Postfix predicates such as `IS NOT NULL` retain their
complete source extent, and explicit parenthesized groups retain their
boundaries.

Form-backed Composition Tables and Metrics keep human Form names in authored
SQL. A submitted Form ID resolves only within readable Forms and must match the
referenced name or stable relation. Ambiguous unbound names fail closed; prior
exact bindings preserve rename identity and reject replacement by a different
Form. Concurrent schema previews and evaluations now wait for the existing query
permit within the same configured timeout instead of failing immediately when
the permit is occupied. The concurrency limit is unchanged.

## Reproduced validation

- Native Domain library: 310 tests passed, including condition mutation,
  source/comment guards, postfix spans, explicit group boundaries, and Undo. SQL
  Document WASM protocol tests: 52 passed. Focused authorized execution, Form
  identity, permit timeout/cancellation, and concurrent empty-Form preview and
  evaluation regressions also passed.
- Related frontend tests: 206 passed across nine files against the release WASM
  adapter. This includes pending-draft safety, failed Apply recovery, focus,
  existing-condition editing, and atomic argument/binding changes.
- The no-entry Form Table creation, save, and reopen browser journey passed five
  consecutive runs against frontend and native source
  `274e2ddb20e47a260a787f1fbd856241e707e908`. This reproduces the prior
  intermittent preview/evaluation failure path; it does not establish the entire
  Form matrix.
- The final Chromium suite passed **16/16 in 6.3 minutes** using frontend
  `56c163da868068457b41f0534f9a4bd4fff3a8af`, native server
  `dfaa7d0a874909ffc7dc2e8f1ca29069906b49f2`, and test source
  `ca2c7c60ecced228928ae0aa1fef51cced91ada7`. It covers shared nested
  conditions, connector changes, typed arguments, exact query results,
  Undo/Redo, removal focus, explicit unused-argument cleanup, save/reopen, Form
  Tables and Metrics, pinned Saved SQL, invalid-input recovery, mobile Text
  focus, and narrow-screen keyboard/touch interaction. Desktop and 390px
  condition screenshots were inspected; supported controls use 16px input text
  and 44px targets.
- Earlier browser runs exposed the postfix-span and focus defects fixed above.
  The preceding broad run passed 15/16; the remaining new SQL journey required
  corrections to its CodeMirror newline, empty-result, and argument-diagnostic
  assertions. Its corrected focused rerun passed before the final 16/16 run.
  These results establish the named paths, not the full acceptance matrix.
- Screenshot review then removed redundant Advanced actions from unsupported
  states. The final production frontend at
  `31b4de19b1b1a4821477eabae3742a29bf6814a4` passed the 206 related frontend
  tests and a focused SQL browser rerun (1/1 in 7.7 seconds). That rerun also
  positively verifies editable Name/Status conditions after reopening saved SQL;
  its 390px screenshot shows one Advanced action. This final render change does
  not alter condition mutations or bindings.
- The production frontend build at `56c163da868068457b41f0534f9a4bd4fff3a8af`
  and native server build at `dfaa7d0a874909ffc7dc2e8f1ca29069906b49f2` passed.
  Later native changes add tests only. Owning-crate Clippy, formatting, Deno
  lint, SQL feature inventory, repository checks, full Mitase, and its quality
  gate passed. Mitase reported zero errors and warnings; the quality gate
  reported zero enforced findings.

Generated Vinxi output was kept outside source inventory during repository
validation; the tooling follow-up is
[#4629](https://github.com/ugoite/ugoite/issues/4629). Full frontend application
source type-checking still reports existing configuration/type errors, tracked
in [#4630](https://github.com/ugoite/ugoite/issues/4630). Configuration checks,
runtime tests, and production builds are separate evidence.

## Remaining acceptance work

Complete admitted-SQL GUI construction, editing, execution, and round-trip proof
remain in [#4565](https://github.com/ugoite/ugoite/issues/4565), including the
operation families listed in the previous checkpoint. Saved/runtime authority,
cardinality, stale-response, revision, history/export, and complete cross-host
matrices remain in [#4613](https://github.com/ugoite/ugoite/issues/4613).
Real-device, screen-reader, theme, and actual 200% zoom evidence remain in
[#4590](https://github.com/ugoite/ugoite/issues/4590). Narrow Chromium viewports
do not constitute real-device proof. Passing this delivery unit does not close
those original acceptance goals.
