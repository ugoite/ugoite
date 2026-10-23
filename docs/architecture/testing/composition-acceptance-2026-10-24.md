---
title: Composition acceptance revalidation — 2026-10-24
---

The merged implementation was pulled from `origin/main` at
`259ceedceaad34549d2fd5cd1c008aa1a1c3102f` and checked again against the
2026-10-23 Composition / SQL Workspace acceptance goals. **Overall: 未達 (not
complete).** This report supplements the
[historical audit](./composition-acceptance-2026-10-23.md); it does not rewrite
that audit's baseline or narrow any goal.

Two remaining defects were reproduced and repaired: Composition's shared
Arguments editor lacked rename/type/removal support, and an invalid or missing
Input stopped independent query branches. Complete executable-SQL GUI coverage,
the strict save/cardinality/partial-Work decision, and full device acceptance
remain open. `coverage_complete` remains `false`.

## Scope and reproducibility

The final product and tests were built and run at
`06cd24a3a32706df31e7190e9e403d8d7e8c0692`, including both repairs and the
argument-dialog focus correction, with Rust 1.95, release WASM, the native
server, and the production frontend on macOS/aarch64. Frontend build metadata
and backend source identity matched that exact commit. The isolated six-case
browser run completed with **6 passed, 0 failed, 0 skipped** in 1.8 minutes.

The earlier product build at `d2490a3c596267eebcae72e64200b191b7dd78d1` recorded
**4 passed / 2 failed**. The two new cases initially used unsuitable label
locators. After selecting actual textbox/combobox roles, a two-case run at
`94570e406558438df2c48f1a39023cb598eafd28` recorded **1 passed / 1 failed**:
argument row replacement lost keyboard focus, so Escape did not reach the
dialog. The shared editor now restores its dialog keyboard owner after
rename/type/removal without taking focus from another selected control. The
final browser run explicitly focuses the type select and verifies that Escape
closes the dialog before rebinding and saving. Earlier failed runs remain failed
evidence, superseded by the rebuilt six-pass run.

Both cases live in files already selected by
`deno task --cwd e2e
composition-studio`: `composition-studio-journey.test.ts`
and `composition-invalid-variable-input.test.ts`. Their required-lane wiring
does not depend on an ad hoc file or a new task.

## Executed evidence

Counts below describe separate runs and must not be added together.

| Check                                                                            | Observed result                                                                       | Evidence boundary                                                                                                             |
| -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| Latest-main `mise run check:repo`                                                | Passed                                                                                | Baseline architecture, generated contracts, Mitase and quality gate                                                           |
| Latest-main `mise run test:frontend:coverage`                                    | 166 files / 1,400 tests passed; protocol 100%                                         | Before the new repairs                                                                                                        |
| Repaired Core `composition_query_evaluation` integration harness                 | 9 passed                                                                              | Selected/transitive input closure; unknown/non-Input/unset rejection; full evaluation stays strict                            |
| Repaired Domain Composition library tests                                        | 70 passed                                                                             | Complete scalar constraints, strict graph, canonicalization and old-source-shaped rejection                                   |
| Repaired Iceberg `test_composition_preview` integration harness                  | 17 passed                                                                             | Authorized saved/draft selection, current schema and exact references, continuation/publication boundaries                    |
| Shared argument DOM/WASM focused suites                                          | 49 passed; declaration-recovery 11 passed; final focus/toolbar/Studio suite 34 passed | Rust rename and comments, type disconnection, used-removal guard, stale edits, recovery without rewriting SQL                 |
| Design/runtime/renderer focused suite                                            | 31 passed                                                                             | Invalid/pending branches, independent updates/pages, stale direct values, repair                                              |
| Combined `mise run test:frontend:coverage`                                       | 167 files / 1,408 tests passed; protocol 100% statements/branches/functions/lines     | Both product repairs; supersedes the baseline frontend count                                                                  |
| Combined `mise run lint:rust`                                                    | Passed                                                                                | Workspace, all targets/features, warnings denied                                                                              |
| Combined `mise run check:repo`                                                   | Passed; Mitase 0 errors; quality gate 0 enforced findings                             | Includes the final browser evidence links; no validator or baseline relaxation                                                |
| `mise run fmt:check`, changed browser Deno lint, facet reports FEAT-COMP-010/011 | Passed                                                                                | Exact evidence links and formatting; planned feature status is retained                                                       |
| Initial rebuilt-product browser run                                              | 4 passed / 2 locator failures                                                         | Reusable creation/save/reopen, Form Table/explicit Metric, responsive modes and invalid-input stale-result suppression passed |
| Final rebuilt-product browser suite                                              | 6 passed / 0 failed / 0 skipped                                                       | All six focused scenarios, including shared arguments save/reopen and independent inputs in both Design and runtime           |

The input-boundary review also reproduced an integer above the safe range
blocking an independent request. The final implementation reuses Domain's
complete value predicate rather than only JSON conversion. Integer overflow,
invalid dates, overlong strings and control characters are omitted only when
unrelated to the selected query closure; selected/full evaluation rejects them.
Unknown identities, invalid graphs, current authorization and result validation
remain strict.

The argument editor retains one default owner: Composition graph Inputs or
constants. Rust owns parameter renaming; SQL text and explicit bindings change
atomically. A stale rename cannot overwrite newer SQL or another editing
session. An incompatible type clears its binding for explicit repair. Directly
typed parameters can be declared from the same Arguments dialog.

## Current goal ledger

`未達` identifies a concrete missing requirement. `未検証` means bounded
implementation and execution evidence exists, but the attachment's complete
acceptance evidence has not been established. A locally repaired defect is not a
declaration that its entire goal has passed.

| ID   | Verdict | Current evidence and remaining boundary                                                                                                                                                              |
| ---- | ------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| G-01 | 未検証  | Strict typed graph passes native/DOM tests; complete cross-surface journey corpus remains necessary.                                                                                                 |
| G-02 | 未検証  | Variable-only saved definitions remain covered by the reusable/Form browser cases; full serializer/history/export corpus remains separate.                                                           |
| G-03 | 未検証  | Rust Form seeds and SQL Document/WASM operations remain the semantic owner; full operation corpus is incomplete.                                                                                     |
| G-04 | 未検証  | Form insertion/explicit aggregation and service identity checks pass; complete rename/recreation/schema browser journey remains.                                                                     |
| G-05 | 未検証  | Explicit arguments, native cardinality and repaired browser editing pass; all browser NULL/zero/multirow/multicolumn/type/ACL branches remain.                                                       |
| G-06 | 未検証  | Definition-only serialization and transient input/results remain; full raw history/export and no-extra-Knowledge evidence remains.                                                                   |
| S-01 | 未検証  | Shared Comp argument editor repaired with actual WASM/DOM and browser persistence evidence; full three-host grammar corpus remains.                                                                  |
| S-02 | 未達    | Executable alias authoring, empty-document construction and DECIMAL precision/scale editing remain GUI gaps; [#4565](https://github.com/ugoite/ugoite/issues/4565).                                  |
| S-03 | 未検証  | Guarded Rust edits and comment-preserving parameter rename pass; complete alternating-edit/invalid-draft/complex SQL corpus remains.                                                                 |
| S-04 | 未達    | Six actions exist; empty SQL has no parser node and no GUI construction route; [#4590](https://github.com/ugoite/ugoite/issues/4590).                                                                |
| S-05 | 未検証  | Rename/type/remove/declaration recovery and explicit rebind repaired; complete standalone default/save/run/Count/page browser corpus remains.                                                        |
| S-06 | 未検証  | Existing invalid-definition preflight and separated plan/run/count/page pass DOM checks; full rebuilt-browser corpus remains.                                                                        |
| D-01 | 未検証  | Two modes and responsive rail/drawer pass the current browser case; all loading/empty/error ownership states remain.                                                                                 |
| D-02 | 未検証  | Current reusable/Form browser cases cover picker insertion and saved references; complete three-path reference-change corpus remains.                                                                |
| D-03 | 未検証  | Shared live renderer and Design/runtime independent-input behavior pass; full result-state corpus remains.                                                                                           |
| D-04 | 未検証  | Current Form browser case checks keyboard layout movement and saved order; full focus/read-order corpus remains.                                                                                     |
| D-05 | 未検証  | Existing direct Text/style tests pass the full frontend lane; all styles and variable Text still need full browser evidence.                                                                         |
| D-06 | 未検証  | Current reusable case and helper tests cover shared values; complete independent rebind/delete/reopen evidence remains.                                                                              |
| R-01 | 未検証  | Native snapshot/authority tests and live Design/runtime cases pass; complete Studio/Design/runtime/authority-change corpus remains.                                                                  |
| R-02 | 未検証  | Independent invalid-input branch defect repaired and tested in both browser modes; complete reversed-response/Space/authority/page corpus remains.                                                   |
| R-03 | 未達    | Save does not execute every runtime cardinality; successful independent Work can accompany another query failure. The strict boundary remains [#4589](https://github.com/ugoite/ugoite/issues/4589). |
| R-04 | 未検証  | Exact-revision and current-authorization paths remain covered; complete copy/restore/history/export preservation remains.                                                                            |
| U-01 | 未検証  | Current creation/Form screenshots and responsive tests show visible insertion actions and live values; all states need holistic inspection.                                                          |
| U-02 | 未検証  | Shared editor, two modes and one Comp default owner are retained; full duplicate-owner audit remains.                                                                                                |
| U-03 | 未検証  | Human-derived variable names and list selection remain; all generated, empty and unavailable states need inspection.                                                                                 |
| U-04 | 未検証  | Current 320/375/390px keyboard/touch cases pass; actual Safari/iOS and actual 200% zoom remain unverified.                                                                                           |
| U-05 | 未検証  | Current focus/Escape/touch cases pass; VoiceOver/NVDA, complete light/dark states and device QA remain [#4590](https://github.com/ugoite/ugoite/issues/4590).                                        |
| B-01 | 未検証  | Native strict rejection passes and no compatibility bridge was added; complete general Knowledge raw-history/export preservation gate remains separate.                                              |
| B-02 | 未検証  | Rust parameter edits and Form seeds remain authoritative; full admitted-operation audit is incomplete.                                                                                               |
| B-03 | 未検証  | Exact revisions, descriptors/cardinality and authorization boundaries remain; complete stale/ACL/schema/revision browser corpus remains.                                                             |

## Delivery and review boundary

The argument repair (`a36fa517a4ce6998aafd413e12a9b8f335540fea`) and
input-isolation repair (`f628625d3e5087ca284021d16bceec9480298606`) start from
the same pulled main and change disjoint files. All ten argument paths and all
eighteen input-isolation paths are byte-identical to the tested integration. The
standalone branch DOM suites passed 34 and 31 tests respectively. The evidence
report is an independent documentation unit. Each has its own independent
review; the input review's Domain-value blocker was corrected before approval.
Pairwise merge checks and exact final heads are recorded with the PRs.

Unrelated gaps remain follow-up work rather than blockers for these scoped
repairs: [#4565](https://github.com/ugoite/ugoite/issues/4565),
[#4589](https://github.com/ugoite/ugoite/issues/4589),
[#4590](https://github.com/ugoite/ugoite/issues/4590), and the broad native-host
fixture/session limitation
[#4595](https://github.com/ugoite/ugoite/issues/4595). The complete broad
browser suite, hosted CI and actual device/assistive- technology gates are not
reported as passing by this local focused audit.
