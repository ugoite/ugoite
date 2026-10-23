---
title: Composition acceptance audit — 2026-10-23
---

This audit evaluates the strict Composition variable graph and SQL Workspace
acceptance goals against `9e28a984a73c67cfe43a5dd0ec7c3dec5d72de8c`. **Overall
result: 未達 (not complete).** Native, WASM and DOM success does not establish
complete executable-SQL GUI coverage or device acceptance.

The criteria remain those of
[ADR-025](../contracts/adr-025-composition-query-variable-graph.md), the
[SQL coverage matrix](../contracts/sql-gui-feature-matrix.md), and the
[Studio](https://github.com/ugoite/ugoite/blob/main/docs/spec/ui/components/composition-studio.yaml)
/
[Workspace](https://github.com/ugoite/ugoite/blob/main/docs/spec/ui/components/sql-workspace.yaml)
contracts. Missing implementation and missing evidence are separate. No
criterion is narrowed to turn a gap into a pass. No goal is declared out of
scope.

## Evidence ledger

The native checks ran on macOS/aarch64 using the repository's Mise environment.
The browser and device gates below remain separate from these checks.

| Check                                                                                          | Observed result | Evidence covered                                                                                                       |
| ---------------------------------------------------------------------------------------------- | --------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `mise run test:composition:domain` on the baseline                                             | 58 passed       | Strict graph, component references, input validation, canonical serialization and old source-shaped document rejection |
| `mise run test:sql-structure:domain` on the baseline                                           | 76 passed       | Native parser and bounded structure proposals; not complete GUI construct coverage                                     |
| `mise run test:composition-adapter` on the baseline                                            | 9 passed        | Portable protocol, server evaluation/save/restore, current authorization and CLI Core/remote parity                    |
| `cargo test -p ugoite-core --test composition_query_evaluation --locked`                       | 6 passed        | Result snapshots, descriptors/cardinality, explicit arguments and stateless continuation requests                      |
| `deno test --allow-read tools/sql_feature_matrix_test.ts tools/sql_aggregate_registry_test.ts` | 7 passed        | Declared registry/matrix and runner wiring; source checks do not execute the executor or GUI                           |
| Focused SQL Document/Toolbar/controller DOM tests                                              | 64 passed       | Existing typed operations and round trips within the tested constructs                                                 |

The corrected continuation regression also ran directly with the owning
integration binary: **1 passed**. The canonical
`v01_knowledge_fixture_is_semantically_recoverable` fixture ran separately: **1
passed**. The baseline adapter count remains nine; these additional checks do
not retroactively change that baseline result.

The original adapter task silently named a retired paging test. This change
selects the actual
`local_graph_table_pages_keep_query_continuations_across_unrelated_input_changes`
test. The baseline's nine passing tests alone did not prove that missing paging
selection. A passing focused test name or merged PR is not a substitute for a
required end-to-end or device check.

## Goal-by-goal baseline verdict

`未達` means a concrete requirement is missing or contradicted. `未検証` means
implementation/evidence exists but the complete required evidence has not been
established by this audit. Evidence is deliberately bounded by the check above.

| ID   | Verdict | Code and observed evidence                                                                                                                                | Remaining boundary                                                                                                                           |
| ---- | ------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| G-01 | 未検証  | `ugoite-domain/src/composition/variable_graph.rs`, `variable_binding.rs`; 58 Domain tests and adapter parity pass                                         | Complete cross-surface journey evidence remains necessary                                                                                    |
| G-02 | 未検証  | Strict component enums and canonicalization in `composition.rs` / `canonical.rs`; old source-shaped components are rejected                               | Inspect final browser-authored documents in the representative journeys                                                                      |
| G-03 | 未達    | Rust SQL Document owns parsing/typed edits, but baseline `CompositionStudio.tsx::resolveForm` generates SELECT/aggregate SQL in JavaScript                | Move the Form seed into Rust; no browser generator fallback                                                                                  |
| G-04 | 未検証  | Form picker normalizes to query variables; service save derives and checks Form identity bindings                                                         | Browser Form insertion, rename/recreation and schema drift together                                                                          |
| G-05 | 未検証  | Explicit argument maps, graph binding, Core scalar result validation; native cardinality tests pass                                                       | All zero/NULL/multirow/multicolumn/type/ACL states in a real browser                                                                         |
| G-06 | 未検証  | `compositionGraphDocumentDraft` selects only definition fields; query evaluation is transient Work                                                        | Full history/export and no-extra-publication journey evidence                                                                                |
| S-01 | 未達    | All hosts use `SQLWorkspace`; Comp argument toolbar omits shared rename/type/remove callbacks                                                             | [#4587](https://github.com/ugoite/ugoite/issues/4587)                                                                                        |
| S-02 | 未達    | `coverage_complete: false`; executable `SELECT 1 AS value` has no active typed alias-authoring operation                                                  | [#4565](https://github.com/ugoite/ugoite/issues/4565); every admitted variant needs construction, editing, semantics and round-trip evidence |
| S-03 | 未検証  | Rust source guards, inverse transactions and typed Document operations; 64 focused DOM checks pass                                                        | Full Native/WASM/DOM corpus, invalid-draft recovery and complex trivia preservation                                                          |
| S-04 | 未達    | Six-action toolbar exists, but empty SQL has no selectable Document root and no GUI construction path                                                     | Empty-document recovery tracked with [#4590](https://github.com/ugoite/ugoite/issues/4590)                                                   |
| S-05 | 未達    | Standalone declaration/default UI exists; unused declarations and invalid defaults do not block baseline Preview/Count                                    | Execution preflight repair plus Comp argument parity [#4587](https://github.com/ugoite/ugoite/issues/4587)                                   |
| S-06 | 未達    | Document/plan/run/count/page operations are separate and page limits are request fields                                                                   | Invalid definition execution gate must cover retained pages and Count too                                                                    |
| D-01 | 未検証  | `CompositionGraphStudio` has Design/Variables modes and a variable rail/mobile drawer                                                                     | Real browser duplicate-owner and navigation checks                                                                                           |
| D-02 | 未検証  | `CompositionVariableBindingPicker` shares Variables/Forms/Saved SQL insertion and reference changes                                                       | Three-path browser journey and exact persisted revision evidence                                                                             |
| D-03 | 未達    | Baseline `CompositionGraphDesignCanvas` renders icons/IDs instead of `CompositionGraphRenderer` values                                                    | Use one semantic renderer for live Design and runtime                                                                                        |
| D-04 | 未達    | Baseline canvas lacks move/reorder controls                                                                                                               | Stable layout editing, keyboard order and focus evidence                                                                                     |
| D-05 | 未達    | Literal editor exists, but renderer collapses four styles into two tags without style distinctions                                                        | Preserve the four styles in the shared renderer; no generic label/tag persistence                                                            |
| D-06 | 未検証  | Component binding changes and deletion preserve shared query variables in the binding helpers                                                             | Multiple-display browser reuse and independent reference changes                                                                             |
| R-01 | 未達    | Core snapshots exist, but baseline Design receives no evaluated values                                                                                    | Connect draft Design to authorized evaluation and retain update base identities                                                              |
| R-02 | 未達    | Runtime parse generations can collide after route reset; invalid inputs also stop unrelated branches                                                      | Route/parse repair plus [#4588](https://github.com/ugoite/ugoite/issues/4588)                                                                |
| R-03 | 未達    | Rust validates save/execute boundaries, but save does not execute all runtime cardinalities; independent successful Work results accompany query failures | Resolve the strict acceptance boundary in [#4589](https://github.com/ugoite/ugoite/issues/4589); do not silently declare it satisfied        |
| R-04 | 未検証  | Exact Saved SQL entry/revision references, current ACL checks, append-only save/restore paths; adapter tests pass                                         | Complete original-update/copy/history/export browser journeys                                                                                |
| U-01 | 未達    | Icon-only Design does not expose the final rendered structure                                                                                             | Real-value canvas repair; holistic browser inspection still required                                                                         |
| U-02 | 未検証  | Two-mode navigation and shared Workspace ownership are present                                                                                            | Inspect all states for repeated visible semantic owners                                                                                      |
| U-03 | 未達    | Baseline Saved SQL-generated variable IDs contain UUIDs in normal rail/picker/canvas lists                                                                | Human-derived new variable names; inspect all empty/loading/list states                                                                      |
| U-04 | 未検証  | Existing E2E has 320/375/390px cases and local-scroll rules                                                                                               | Execute those cases; actual iOS/200% zoom evidence remains separate                                                                          |
| U-05 | 未達    | Picker loading/empty states can be blank; icon/focus/save infrastructure exists                                                                           | [#4590](https://github.com/ugoite/ugoite/issues/4590), including actual assistive technology and light/dark checks                           |
| B-01 | 未検証  | Strict `deny_unknown_fields`, old source-shaped fixture rejection, removed-operation protocol tests pass                                                  | Full raw-history/export preservation evidence is a separate gate; no compatibility path may be added                                         |
| B-02 | 未達    | Form SELECT/aggregate template generation exists in baseline browser code                                                                                 | Rust Form seed repair; retain one SQL authority                                                                                              |
| B-03 | 未検証  | Native result/cardinality and exact-revision paths exist; no first-row conversion is used by the graph renderer                                           | Browser stale/ACL/schema/revision journeys and remaining branch isolation                                                                    |

## Corrective delivery units

Each unit starts from the same baseline and changes disjoint files. These are
partial corrective changes, not a release or full-acceptance declaration.

1. Runtime input parsing and retained-page authority: route-scoped parse epochs,
   cache clearing on errors/authority changes, and no replay of an old cursor.
   Focused runtime suite: 10 passed.
   [PR #4591](https://github.com/ugoite/ugoite/pull/4591).
2. Standalone SQL definition preflight: unused/undeclared arguments and invalid
   defaults invalidate plan/results/continuations and block plan/run/count.
   Focused controller/new/saved SQL suite: 31 passed.
   [PR #4592](https://github.com/ugoite/ugoite/pull/4592).
3. Live Design and layout: shared renderer, transient draft evaluation, stable
   reordering/row movement, direct Text styles, Rust-owned Form SQL seeds, and
   human-derived Saved SQL variable IDs. Domain suite: 60 passed; focused
   protocol/Studio/renderer/controller suite: 107 passed, followed by the final
   expanded controller suite: 8 passed. Full frontend suite: **166 files / 1,395
   tests passed**, with protocol coverage **100%
   statements/branches/functions/lines**.
   [PR #4594](https://github.com/ugoite/ugoite/pull/4594).
4. This audit and focused adapter selector repair.
   [PR #4593](https://github.com/ugoite/ugoite/pull/4593).

Independent code reviews cover the exact corrective commits. They do not replace
hosted CI, browser journeys or device QA. Outstanding scope is tracked in
[#4565](https://github.com/ugoite/ugoite/issues/4565),
[#4587](https://github.com/ugoite/ugoite/issues/4587),
[#4588](https://github.com/ugoite/ugoite/issues/4588),
[#4589](https://github.com/ugoite/ugoite/issues/4589), and
[#4590](https://github.com/ugoite/ugoite/issues/4590).

## Representative scenarios

| Scenario                                                           | Audit status                                                                                                                          |
| ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------- |
| Form → Table → reorder → save → reopen                             | Passed in the isolated three-case Studio browser run (including keyboard movement and saved layout)                                   |
| Form → Metric → explicit aggregate → saved variable-only document  | Passed in the isolated Studio run; saved documents contain variable-only bindings and explicit aggregation                            |
| Saved SQL exact revision, reuse, original change and explicit copy | Reuse/save/reopen and explicit copy/count browser cases passed; complete broad-run acceptance remains open                            |
| Inputs/constants → dependent queries only                          | Focused DOM/controller evidence exists; invalid-independent-branch gap remains #4588                                                  |
| Alternating GUI/text edits with all admitted constructs            | Not complete, #4565                                                                                                                   |
| Complex SQL with comments/aliases and untouched clauses            | Bounded round-trip evidence exists; complete corpus not established                                                                   |
| Invalid SQL/arguments/cardinality → local errors → repair          | Focused tests pass for the repaired definition gate; browser coverage still required                                                  |
| Reversed replies/Space switch/ACL/schema changes                   | Focused regressions and native authorization tests pass; complete browser run pending                                                 |
| 320/375/390px, Safari, VoiceOver/NVDA, actual 200% zoom            | Responsive keyboard/touch case and the Form Design case at 320/375/390px passed; device/assistive-technology checks remain unverified |
| Old Comp rejection and released raw history/export preservation    | Native strict rejection passes; complete preservation gate remains unverified                                                         |

A future completion report must attach the required execution results for every
remaining row. Passing these corrective PRs must not flip `coverage_complete` or
turn missing evidence into an achieved requirement.

The initial broad native-host browser run hit the existing Golden Journey's
300-second `beforeAll` fixture timeout. Later fixtures that create Spaces then
received `RECENT_PASSKEY_REQUIRED` after the five-minute privileged session
window expired. These are reported in
[#4595](https://github.com/ugoite/ugoite/issues/4595), separately from product
assertions; the suite must not be reported as passing.

## Final bounded browser and integration evidence

The broad native-host run completed with **7 passed / 6 failed**. Two failures
were test expectations for the newly readable generated variable headings and an
ambiguous metric locator; both assertions were corrected without weakening the
value checks. The remaining fixture/session failures are tracked in #4595. The
broad suite is not a passing gate.

An isolated run then passed **3 tests, 0 failures, 0 skipped**: reusable tool
creation/save/reopen, Form-backed Table/explicit Metric with live Design and
keyboard layout movement, and responsive keyboard/touch interaction. It used the
unchanged product build from `a69901049d88014ad63d52c734cb82a5ccfaee74` with the
corrected test assertions at `85c51040875b5f34bec00515e2017bdbff128c5d`. These
results do not stand in for the unresolved broad or device gates.

All four corrective branches also merged together locally without conflicts. The
combined frontend gate passed **166 files / 1,399 tests**, with **100%**
protocol statements, branches, functions and lines. The individual OpenAPI,
operation registry, architecture, Space compatibility, release authority,
current-stack, supported-contract and legacy-auth checks passed, as did
formatting and native/WASM Composition parity.

The first aggregate `mise run check:repo` failed with 3,205 Mitase diagnostics.
Independent baseline/change isolation traced this to two new test type queries
that the pinned TSX inventory parser rejected. This was caused by the Design
change, rather than unrelated specification drift. Equivalent type-only module
imports preserve the test behavior without weakening inventory or validation.
After that correction, the complete `mise run check:repo` passed with **zero
Mitase errors**, and the facet suites passed **7 tests**. The regression and
resolution are tracked in [#4596](https://github.com/ugoite/ugoite/issues/4596).
The earlier failed run remains failed evidence; it is superseded by this actual
successful rerun.

The creation UX was also checked separately: the four insertion actions stay
visible, Save is separate from the mode control, the name field has a
placeholder, and only explicit selection opens the Inspector. Empty picker
panels announce local state. Narrow tables keep readable cells inside their own
horizontal scroll region. The updated three-case browser run passed at
integrated `99c895fee5fd2f3ddff2b274e228f5661b774098`, and full frontend
coverage again passed **166 files / 1,399 tests** with protocol coverage at
**100%**. The final rebuilt product, including focus, mobile Inspector and
parser corrections, also passed **3 tests, 0 failures, 0 skipped** at integrated
`83e8171b06e094ff3f975c4c01e5dca202140e06`. The final mock suite passed **24
tests**; facet verification passed **7 tests**. Visual inspection at 390px
confirmed the Inspector follows the table and does not cover result rows. The
tools task passed **172 tests, 0 failed, 1 ignored**; the ignored test is not
counted as evidence.

The audit's two YAML contract links now point to their repository sources; the
published docsite navigation and internal-link checks passed **2 tests, 0
skipped**.

Hosted pull-request CI remains a separate gate. A smoke/mobile lane exceeded its
job window while installing browser system dependencies, before test execution;
this is tracked in [#4597](https://github.com/ugoite/ugoite/issues/4597). It
does not establish a product assertion failure or a passing browser lane.
