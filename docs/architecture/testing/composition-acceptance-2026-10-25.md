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

This checkpoint was reproduced against integrated head
`6c20da665b47279de58b4a9480b2944475a936c1`. The latest product snapshot
exercised below is `cb9af356d6cece55a969e4cb6ada231dbbd4265b`. Final focused
browser runs below exercise the mobile wait adjustment and translated argument
controls.

## Evidence and boundaries

Environment: macOS/aarch64, Rust 1.95, local native and production frontend
builds. Exact commits and commands below identify reproduced lanes; hosted CI
and real-device findings are not inferred from local results.

- Native build and production frontend build passed with source SHA
  `6c20da665b47279de58b4a9480b2944475a936c1`.
- Root gates reproduced at `6c20da6`: `mise run fmt:check` passed (Deno checked
  65 files); `mise run lint:rust` passed
  (`cargo clippy --workspace --all-targets --all-features -- -D warnings`);
  `mise run check:repo` passed, including Mitase with 0 errors/0 warnings and
  the quality gate with 0 enforced findings.
- The 12-case Chromium selection at product/test source SHA `6c20da6` had **11
  passed and 1 failed**. The failure was the 5-second default wait for the Saved
  SQL Metric dialog to close in “unnamed mobile Composition keeps Text focus and
  inserts existing data before naming”; the value assertion already had a
  30-second wait. The command was the 12-case Chromium selection of
  `sql-columns-editor.test.ts`, `composition-studio-journey.test.ts`, and
  `composition-invalid-variable-input.test.ts`.
- An unchanged single-case rerun at `6c20da6` passed **1/1 in 20.8 seconds**. It
  also passed the 16px text focus, FormTable and Metric, Saved SQL Metric, save,
  and reopen assertions. This establishes a flaky bounded wait at that source
  head, not a deterministic fix or complete browser acceptance. The current
  follow-up extends only the dialog-closure assertion to 30 seconds. The changed
  case subsequently passed in 26.4 seconds against the newly built native server
  and production frontend at `cb9af356d6cece55a969e4cb6ada231dbbd4265b`.
- Frontend/spec delta based on `d8acd3725` passed **36 tests across 4 files**,
  `mise run fmt:check` (65 files), SQL matrix (6/6), and Mitase. These are
  focused checks; the later browser results are separate evidence and neither
  establishes full acceptance.
- At source `cb9af356d6cece55a969e4cb6ada231dbbd4265b`, the production frontend
  build passed, release WASM tests passed **157/157 across 6 files**, native SQL
  document tests passed **108/108**, authorized CASE and alias execution passed
  **2/2**, the SQL matrix passed **6/6**, and `mise run fmt:check` checked 65
  files. These are bounded checks rather than complete acceptance.
- PR [#4625](https://github.com/ugoite/ugoite/pull/4625) adds typed CAST,
  Boolean NOT, binary bitwise/shift operators, and CASE support; its independent
  review approved the change and its apply-path bugs were resolved. PR
  [#4626](https://github.com/ugoite/ugoite/pull/4626) adds translated argument
  controls and a qualified-alias guard. Its browser journey passed 1/1 in 3.6
  seconds after correcting test readiness and role locators. It creates an
  output alias and an integer argument with default 10, verifies Undo/Redo,
  executes against value 11, and saves/reopens both SQL and declarations.
  Product source is `cb9af356d6cece55a969e4cb6ada231dbbd4265b`; the final test
  is in `1bc747194174d1508091f2bbe13f753a414d9f19`.
- The final native binary build and `mise run check:repo` passed. Owning-crate
  Clippy passed with
  `cargo clippy -p ugoite-domain -p ugoite-wasm --all-targets
  -- -D warnings`.
  Initial selector/contract errors were corrected by collecting the actual
  editor/test files and linking exact UI verification; full Mitase then passed
  with 0 errors/0 warnings and the quality gate had 0 enforced findings. The
  final D head is `de339d77ad20408d0bd1afc1e4529b21dcb7e4fa`; its last source
  adjustment only initializes the CASE token struct directly to satisfy Clippy.
  Hosted CI remains a separate gate.
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
- `mise run test:older-cli-knowledge-preservation` passed 1/1 using the
  integrity-verified v0.2.1 CLI from source
  `51f469e3f81113e4295b8d579410c538fd36a61c`. It proves that this older CLI
  updates unrelated Knowledge without rewriting a Composition; it does not
  establish the full raw-history and export preservation corpus.
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

S-02 remains `未達`. PRs [#4621](https://github.com/ugoite/ugoite/pull/4621),
[#4622](https://github.com/ugoite/ugoite/pull/4622) add independently reviewed
typed-filter arguments and projection aliases. Rich primitive support is tracked
separately in [#4623](https://github.com/ugoite/ugoite/issues/4623) and
[#4625](https://github.com/ugoite/ugoite/pull/4625); bounded authoring and CASE
execution evidence now exists, while per-operator execution proof remains
incomplete. Remaining GUI evidence gaps include aggregate `FILTER` and ordered
arguments; grouping sets, `ROLLUP`, `CUBE`, and `ALL`; complete authoring and
execution evidence for rich primitive attributes, arrays, structs, and access
expressions; planner intrinsic admission such as `EXTRACT` and `SUBSTRING`;
`ILIKE`, `SIMILAR`, `IS DISTINCT
FROM`, and truth predicates; typed date,
timestamp, interval, and hexadecimal values; semi/anti joins; `LIMIT`
expressions; CTE and derived-column aliases; and beginner nested filter groups.
These gaps are not claims that the core executor rejects those constructs;
executor admission and GUI reachability are separate evidence questions. Source
retention alone does not prove GUI authoring coverage.

## Goal ledger

`達成` requires the complete criterion and its prescribed evidence. `未検証`
means partial implementation or bounded tests exist, but complete acceptance has
not been established. `未達` records a known missing requirement. These are
individual goal verdicts, not a feature-level pass count.

| Goal | Verdict | Evidence boundary / known gap                                                                                                                                                                                                                                                                                                                                                          |
| ---- | ------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| G-01 | 未検証  | Typed graph and selected evaluation have focused checks; complete cross-surface journey remains.                                                                                                                                                                                                                                                                                       |
| G-02 | 未検証  | Variable-backed displays are covered in focused paths; serializer/history/export corpus remains.                                                                                                                                                                                                                                                                                       |
| G-03 | 未検証  | Rust owns SQL semantics in tested paths; full admitted-operation corpus remains.                                                                                                                                                                                                                                                                                                       |
| G-04 | 未検証  | Form binding and explicit aggregation have focused evidence; rename/recreation/schema journeys remain.                                                                                                                                                                                                                                                                                 |
| G-05 | 未検証  | Strict cardinality and explicit bindings have focused evidence; full browser NULL/row/column/type/ACL matrix remains.                                                                                                                                                                                                                                                                  |
| G-06 | 未検証  | Definition/work separation has focused checks; full history/export/no-extra-Knowledge proof remains.                                                                                                                                                                                                                                                                                   |
| S-01 | 未検証  | Shared workspace paths tested; complete three-host grammar corpus remains.                                                                                                                                                                                                                                                                                                             |
| S-02 | 未達    | Typed-filter arguments, projection aliases, and selected typed expressions have bounded PR evidence, but complete GUI authoring and execution evidence remains open for the SQL forms listed above, including nested filter groups and rich primitive attributes; see [#4565](https://github.com/ugoite/ugoite/issues/4565) and [#4623](https://github.com/ugoite/ugoite/issues/4623). |
| S-03 | 未検証  | Guarded SQL edits have focused checks; alternating edits, invalid drafts, comments, and complex SQL corpus remains.                                                                                                                                                                                                                                                                    |
| S-04 | 未検証  | Actions are present, and empty-SQL construction has implementation coverage; six-operation reachability from every required SQL starting shape has not been proven and remains tracked in [#4613](https://github.com/ugoite/ugoite/issues/4613).                                                                                                                                       |
| S-05 | 未検証  | Inline argument creation has Studio DOM evidence in [#4621](https://github.com/ugoite/ugoite/pull/4621), and rename/type/recovery have focused checks; the complete typed/default/save/run/Count/page matrix remains unverified.                                                                                                                                                       |
| S-06 | 未検証  | Plan/run/count/page boundaries have focused checks; integrated browser suite remains incomplete.                                                                                                                                                                                                                                                                                       |
| D-01 | 未検証  | Two-mode layout has responsive coverage; complete state/ownership inspection remains.                                                                                                                                                                                                                                                                                                  |
| D-02 | 未検証  | Picker and Form paths have focused/browser evidence; complete reference-change paths remain.                                                                                                                                                                                                                                                                                           |
| D-03 | 未検証  | Shared renderer and selected updates have focused evidence; complete result-state corpus remains.                                                                                                                                                                                                                                                                                      |
| D-04 | 未検証  | Reordering has focused coverage; full focus and reading-order proof remains.                                                                                                                                                                                                                                                                                                           |
| D-05 | 未検証  | Direct text/style behavior has focused coverage; complete browser style and variable-text paths remain.                                                                                                                                                                                                                                                                                |
| D-06 | 未検証  | Shared values are covered in selected paths; complete rebind/delete/reopen evidence remains.                                                                                                                                                                                                                                                                                           |
| R-01 | 未検証  | Snapshot/authority tests and browser paths exist; complete Studio/Design/runtime authority-change journey remains.                                                                                                                                                                                                                                                                     |
| R-02 | 未検証  | Independent invalid-input branches have bounded evidence; reversed-response, Space-switch, authority, and paging corpus remains.                                                                                                                                                                                                                                                       |
| R-03 | 未検証  | Strict-save checks cover selected validation paths; complete runtime cardinality and partial-Work boundary is not accepted.                                                                                                                                                                                                                                                            |
| R-04 | 未検証  | Exact revision and authorization paths have focused checks; copy/restore/history/export preservation remains.                                                                                                                                                                                                                                                                          |
| U-01 | 未検証  | Some creation and responsive journeys are exercised; holistic review across all states remains.                                                                                                                                                                                                                                                                                        |
| U-02 | 未検証  | Shared editor and mode ownership have focused review; complete duplicate-owner audit remains.                                                                                                                                                                                                                                                                                          |
| U-03 | 未検証  | Human-readable names and selection paths exist; generated/empty/unavailable states remain to inspect.                                                                                                                                                                                                                                                                                  |
| U-04 | 未検証  | Automated narrow-width checks exist; real device and 200% zoom evidence remains open in [#4590](https://github.com/ugoite/ugoite/issues/4590).                                                                                                                                                                                                                                         |
| U-05 | 未検証  | Keyboard/focus paths have focused evidence; VoiceOver/NVDA, themes, and device QA remain open in [#4590](https://github.com/ugoite/ugoite/issues/4590).                                                                                                                                                                                                                                |
| B-01 | 未検証  | Strict rejection and one older-CLI unrelated-Knowledge preservation case have focused evidence; the general Knowledge raw-history/export corpus remains.                                                                                                                                                                                                                               |
| B-02 | 未検証  | Rust SQL ownership is covered on selected paths; complete admitted-operation audit remains.                                                                                                                                                                                                                                                                                            |
| B-03 | 未検証  | Exact revision, descriptor, and authorization checks exist; complete stale/ACL/schema/revision browser corpus remains.                                                                                                                                                                                                                                                                 |

## Reproduction index

Run from the repository root with Rust 1.95. The evidence above distinguishes
the baseline, the product snapshot, and later test/spec corrections.

- `mise run fmt:check`, `mise run check:repo`, and `mise run build:frontend`
  passed. The baseline additionally passed the full `mise run lint:rust` lane.
- `cargo build -p ugoite-server --locked` passed.
- `cargo test -p ugoite-domain sql_document::tests --locked` passed 108 tests;
  `cargo test -p ugoite-iceberg --test authorized_query _edit_executes --locked`
  passed 2 tests.
- `cargo clippy -p ugoite-domain -p ugoite-wasm --all-targets -- -D warnings`
  passed after the CASE struct initialization repair.
- The focused frontend lane used `deno task --cwd frontend test --run` with the
  Columns, document tree, actual-WASM tree, filter/sort, Composition graph
  arguments, and protocol test files; all 157 tests passed.
- `deno test -A tools/sql_feature_matrix_test.ts` passed 6 tests. Full Mitase
  passed with 0 errors/0 warnings after the exact evidence and inventory repair;
  `mitase:quality-gate` reported 0 enforced findings.
- The focused Chromium lane used `deno task --cwd e2e full` with the isolated
  owner fixture. The changed mobile authoring case passed at product/test
  snapshot `cb9af356d6cece55a969e4cb6ada231dbbd4265b`; the final typed-argument
  test from `1bc747194174d1508091f2bbe13f753a414d9f19` passed separately against
  that product snapshot. Earlier failed runs remain separate evidence above.
- `mise run test:older-cli-knowledge-preservation` passed 1/1 using the pinned
  v0.2.1 CLI source SHA `51f469e3f81113e4295b8d579410c538fd36a61c`. This single
  preservation case is not complete B-01 evidence.
