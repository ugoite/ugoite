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
[Studio](../../spec/ui/components/composition-studio.yaml) /
[Workspace](../../spec/ui/components/sql-workspace.yaml) contracts. Missing
implementation and missing evidence are separate. No criterion is narrowed to
turn a gap into a pass. No goal is declared out of scope.

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
   [#4583](https://github.com/ugoite/ugoite/issues/4583).
2. Standalone SQL definition preflight: unused/undeclared arguments and invalid
   defaults invalidate plan/results/continuations and block plan/run/count.
   Focused controller/new/saved SQL suite: 31 passed.
   [#4584](https://github.com/ugoite/ugoite/issues/4584).
3. Live Design and layout: shared renderer, transient draft evaluation, stable
   reordering/row movement, direct Text styles, Rust-owned Form SQL seeds, and
   human-derived Saved SQL variable IDs. Domain suite: 60 passed; focused
   protocol/Studio/renderer/controller suite: 107 passed, followed by the final
   expanded controller suite: 8 passed. Full frontend suite: **166 files / 1,395
   tests passed**, with protocol coverage **100%
   statements/branches/functions/lines**.
   [#4585](https://github.com/ugoite/ugoite/issues/4585).
4. This audit and focused adapter selector repair.
   [#4586](https://github.com/ugoite/ugoite/issues/4586).

Independent code reviews cover the exact corrective commits. They do not replace
hosted CI, browser journeys or device QA. Outstanding scope is tracked in
[#4565](https://github.com/ugoite/ugoite/issues/4565),
[#4587](https://github.com/ugoite/ugoite/issues/4587),
[#4588](https://github.com/ugoite/ugoite/issues/4588),
[#4589](https://github.com/ugoite/ugoite/issues/4589), and
[#4590](https://github.com/ugoite/ugoite/issues/4590).

## Representative scenarios

| Scenario                                                           | Audit status                                                                                 |
| ------------------------------------------------------------------ | -------------------------------------------------------------------------------------------- |
| Form → Table → reorder → save → reopen                             | Added to the real browser Studio journey; result pending                                     |
| Form → Metric → explicit aggregate → saved variable-only document  | Existing browser journey extended to assert actual Design and runtime values; result pending |
| Saved SQL exact revision, reuse, original change and explicit copy | Code/native evidence exists; complete browser run pending                                    |
| Inputs/constants → dependent queries only                          | Focused DOM/controller evidence exists; invalid-independent-branch gap remains #4588         |
| Alternating GUI/text edits with all admitted constructs            | Not complete, #4565                                                                          |
| Complex SQL with comments/aliases and untouched clauses            | Bounded round-trip evidence exists; complete corpus not established                          |
| Invalid SQL/arguments/cardinality → local errors → repair          | Focused tests pass for the repaired definition gate; browser coverage still required         |
| Reversed replies/Space switch/ACL/schema changes                   | Focused regressions and native authorization tests pass; complete browser run pending        |
| 320/375/390px, Safari, VoiceOver/NVDA, actual 200% zoom            | Automated narrow-width run pending; device/assistive-technology checks unverified            |
| Old Comp rejection and released raw history/export preservation    | Native strict rejection passes; complete preservation gate remains unverified                |

A future completion report must attach the required execution results for every
remaining row. Passing these corrective PRs must not flip `coverage_complete` or
turn missing evidence into an achieved requirement.
