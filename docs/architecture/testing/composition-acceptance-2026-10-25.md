---
title: Composition / SQL Workspace acceptance checkpoint — 2026-10-25
---

# Acceptance checkpoint: Composition / SQL Workspace

**Overall: 未達 (not complete). `coverage_complete`: false.** This checkpoint
supplements the
[2026-10-24 revalidation](https://github.com/ugoite/ugoite/blob/main/docs/architecture/testing/composition-acceptance-2026-10-24.md)
and the original
[goals and acceptance criteria](https://github.com/ugoite/ugoite/blob/main/docs/architecture/testing/composition-acceptance-2026-10-23.md).
It does not revise or narrow them. All 30 goal IDs below remain in scope.

This checkpoint was reproduced against the integrated existing-PR head
`6c20da665b47279de58b4a9480b2944475a936c1`. A separate parent-owned
frontend/spec delta at `d8acd3725` has focused test evidence listed below; it
does not change Rust or WASM. The follow-up browser wait adjustment in this
change is E2E test code only and has not been run at its changed test-source
head. Existing public PRs are outside this checkpoint's write scope.

## Evidence and boundaries

Environment: macOS/aarch64, Rust 1.95, local native and production frontend
builds. Exact commits and commands below identify reproduced lanes; hosted CI
and real-device findings are not inferred from local results.

- Native build and production frontend build passed with source SHA
  `6c20da665b47279de58b4a9480b2944475a936c1`; logs are
  `/tmp/ugoite-comp-followup-20261025/baseline-native-build.log` and
  `baseline-production-build.log`.
- Root gates reproduced at `6c20da6`: `mise run fmt:check` passed (Deno checked
  65 files); `mise run lint:rust` passed
  (`cargo clippy --workspace --all-targets --all-features -- -D warnings`);
  `mise run check:repo` passed, including Mitase with 0 errors/0 warnings and
  the quality gate with 0 enforced findings. Exact command outputs are
  summarized in
  `/tmp/ugoite-comp-completion-20261024/followup-gates-handoff-20261025.md`.
- The 12-case Chromium selection at product/test source SHA `6c20da6` had **11
  passed and 1 failed**. The failure was the 5-second default wait for the Saved
  SQL Metric dialog to close in “unnamed mobile Composition keeps Text focus and
  inserts existing data before naming”; the value assertion already had a
  30-second wait. Log and JUnit are
  `/tmp/ugoite-comp-followup-20261025/baseline-chromium-browser.log` and
  `baseline-chromium-junit.xml`.
- An unchanged single-case rerun at `6c20da6` passed **1/1 in 20.8 seconds**. It
  also passed the 16px text focus, FormTable and Metric, Saved SQL Metric, save,
  and reopen assertions. This establishes a flaky bounded wait at that source
  head, not a deterministic fix or complete browser acceptance. The current
  follow-up extends only the dialog-closure assertion to 30 seconds; its changed
  test-source head still needs execution.
- Parent-owned frontend/spec delta `d8acd3725` passed **36 tests across 4
  files**, `mise run fmt:check` (65 files), SQL matrix (6/6), and Mitase. These
  are exact-head focused checks; they do not verify the pending new-feature
  browser path or full acceptance.
- `strict-save-composition-after-filter-stackfix`: 35 Iceberg library tests and
  21 Composition tests passed, with matching query tests. Strict-save focused
  evidence does not establish every browser cardinality or failure-state branch.
- Human Form/API/Studio/Composition focused lanes recorded 49, 17, 25, and 32
  passing tests respectively, plus one rename/pin browser path. Filter/sort
  focused coverage recorded 12 frontend matrix cases and 6 Domain cases; Rust
  clippy and Mitase checks passed with 0 quality findings.
- Earlier typed API, CLI, and native/WASM parity lanes passed 47 API tests, 1
  CLI test, and the recorded parity check. These do not substitute for
  end-to-end coverage.
- Earlier focused Rust, frontend, and WASM evidence remains bounded to the named
  paths in the previous checkpoint and the original acceptance reports. It does
  not substitute for the full acceptance matrices below. Failed runs remain
  recorded; a rerun only updates evidence for that same case.

Open follow-up tracking includes
[#4619](https://github.com/ugoite/ugoite/issues/4619) (automated acceptance
evidence), [#4613](https://github.com/ugoite/ugoite/issues/4613) (remaining
acceptance matrix), [#4565](https://github.com/ugoite/ugoite/issues/4565) (SQL
GUI coverage), [#4590](https://github.com/ugoite/ugoite/issues/4590)
(device/accessibility/zoom/theme acceptance), and
[#4595](https://github.com/ugoite/ugoite/issues/4595) (historical native
fixture/session limitation). No real iOS device, VoiceOver/NVDA, or actual 200%
zoom proof is available; automated WebKit is not device proof.

S-02 remains `未達`. Published PRs
[#4621](https://github.com/ugoite/ugoite/pull/4621) and
[#4622](https://github.com/ugoite/ugoite/pull/4622) add independently reviewed
typed-filter arguments and projection aliases. Rich primitive support is tracked
separately in [#4623](https://github.com/ugoite/ugoite/issues/4623); its
implementation and runtime proof are still pending. Remaining GUI evidence gaps
include aggregate `FILTER` and ordered arguments; grouping sets, `ROLLUP`,
`CUBE`, and `ALL`; arrays, structs, and access expressions; planner intrinsic
admission such as `EXTRACT` and `SUBSTRING`; `ILIKE`, `SIMILAR`,
`IS DISTINCT
FROM`, and truth predicates; typed date, timestamp, interval, and
hexadecimal values; semi/anti joins; `LIMIT` expressions; CTE and derived-column
aliases; and beginner nested filter groups. These gaps are not claims that the
core executor rejects those constructs; executor admission and GUI reachability
are separate evidence questions. Source retention alone does not prove GUI
authoring coverage.

## Goal ledger

`達成` requires the complete criterion and its prescribed evidence. `未検証`
means partial implementation or bounded tests exist, but complete acceptance has
not been established. `未達` records a known missing requirement. These are
individual goal verdicts, not a feature-level pass count.

| Goal | Verdict | Evidence boundary / known gap                                                                                                                                                                                                                                                                                                     |
| ---- | ------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| G-01 | 未検証  | Typed graph and selected evaluation have focused checks; complete cross-surface journey remains.                                                                                                                                                                                                                                  |
| G-02 | 未検証  | Variable-backed displays are covered in focused paths; serializer/history/export corpus remains.                                                                                                                                                                                                                                  |
| G-03 | 未検証  | Rust owns SQL semantics in tested paths; full admitted-operation corpus remains.                                                                                                                                                                                                                                                  |
| G-04 | 未検証  | Form binding and explicit aggregation have focused evidence; rename/recreation/schema journeys remain.                                                                                                                                                                                                                            |
| G-05 | 未検証  | Strict cardinality and explicit bindings have focused evidence; full browser NULL/row/column/type/ACL matrix remains.                                                                                                                                                                                                             |
| G-06 | 未検証  | Definition/work separation has focused checks; full history/export/no-extra-Knowledge proof remains.                                                                                                                                                                                                                              |
| S-01 | 未検証  | Shared workspace paths tested; complete three-host grammar corpus remains.                                                                                                                                                                                                                                                        |
| S-02 | 未達    | Typed-filter arguments and projection aliases have bounded new PR evidence, but complete GUI coverage remains open for the SQL forms listed above, including nested filter groups and rich primitive values; see [#4565](https://github.com/ugoite/ugoite/issues/4565) and [#4623](https://github.com/ugoite/ugoite/issues/4623). |
| S-03 | 未検証  | Guarded SQL edits have focused checks; alternating edits, invalid drafts, comments, and complex SQL corpus remains.                                                                                                                                                                                                               |
| S-04 | 未達    | Actions are present, and empty-SQL construction has implementation coverage; complete six-operation reachability from every required SQL starting shape remains unproven and tracked in [#4613](https://github.com/ugoite/ugoite/issues/4613).                                                                                    |
| S-05 | 未達    | Argument rename/type/recovery is repaired in focused tests; inline argument creation and complete typed/default/save/run/Count/page evidence remain.                                                                                                                                                                              |
| S-06 | 未検証  | Plan/run/count/page boundaries have focused checks; integrated browser suite remains incomplete.                                                                                                                                                                                                                                  |
| D-01 | 未検証  | Two-mode layout has responsive coverage; complete state/ownership inspection remains.                                                                                                                                                                                                                                             |
| D-02 | 未検証  | Picker and Form paths have focused/browser evidence; complete reference-change paths remain.                                                                                                                                                                                                                                      |
| D-03 | 未検証  | Shared renderer and selected updates have focused evidence; complete result-state corpus remains.                                                                                                                                                                                                                                 |
| D-04 | 未検証  | Reordering has focused coverage; full focus and reading-order proof remains.                                                                                                                                                                                                                                                      |
| D-05 | 未検証  | Direct text/style behavior has focused coverage; complete browser style and variable-text paths remain.                                                                                                                                                                                                                           |
| D-06 | 未検証  | Shared values are covered in selected paths; complete rebind/delete/reopen evidence remains.                                                                                                                                                                                                                                      |
| R-01 | 未検証  | Snapshot/authority tests and browser paths exist; complete Studio/Design/runtime authority-change journey remains.                                                                                                                                                                                                                |
| R-02 | 未検証  | Independent invalid-input branches have bounded evidence; reversed-response, Space-switch, authority, and paging corpus remains.                                                                                                                                                                                                  |
| R-03 | 未検証  | Strict-save checks cover selected validation paths; complete runtime cardinality and partial-Work boundary is not accepted.                                                                                                                                                                                                       |
| R-04 | 未検証  | Exact revision and authorization paths have focused checks; copy/restore/history/export preservation remains.                                                                                                                                                                                                                     |
| U-01 | 未検証  | Some creation and responsive journeys are exercised; holistic review across all states remains.                                                                                                                                                                                                                                   |
| U-02 | 未検証  | Shared editor and mode ownership have focused review; complete duplicate-owner audit remains.                                                                                                                                                                                                                                     |
| U-03 | 未検証  | Human-readable names and selection paths exist; generated/empty/unavailable states remain to inspect.                                                                                                                                                                                                                             |
| U-04 | 未検証  | Automated narrow-width checks exist; real device and 200% zoom evidence remains open in [#4590](https://github.com/ugoite/ugoite/issues/4590).                                                                                                                                                                                    |
| U-05 | 未検証  | Keyboard/focus paths have focused evidence; VoiceOver/NVDA, themes, and device QA remain open in [#4590](https://github.com/ugoite/ugoite/issues/4590).                                                                                                                                                                           |
| B-01 | 未検証  | Strict rejection has focused native evidence; general Knowledge history/export preservation gate remains.                                                                                                                                                                                                                         |
| B-02 | 未検証  | Rust SQL ownership is covered on selected paths; complete admitted-operation audit remains.                                                                                                                                                                                                                                       |
| B-03 | 未検証  | Exact revision, descriptor, and authorization checks exist; complete stale/ACL/schema/revision browser corpus remains.                                                                                                                                                                                                            |

## Reproduction index

Commands are root `mise` tasks or the repository's focused lanes; run from the
repository root. At `6c20da6`, the reproduced root gates were
`mise run fmt:check`, `mise run lint:rust`, and `mise run check:repo`, all
passing. Native/production build, 12-case browser, and unchanged single-case
reproduction logs are under `/tmp/ugoite-comp-followup-20261025/`. Parent-owned
delta `d8acd3725` logs are `filter-integrated-format.log`,
`filter-integrated-matrix.log`, `filter-integrated-mitase.log`, and
`filter-integrated-tests.log` in the same directory. The 12-case browser
selection was `sql-columns-editor.test.ts`,
`composition-studio-journey.test.ts`, and
`composition-invalid-variable-input.test.ts`; its failure and the later
single-case pass are recorded above. The focused CLI Knowledge-preservation gate
is still running as of this report and is not counted as passed.
