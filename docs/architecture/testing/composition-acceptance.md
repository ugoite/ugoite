---
title: "SQL-first Composition acceptance plan"
---

This matrix tracks the unreleased typed query graph and its restored Browser
workflows. It records implementation and validation evidence; it does not
announce or authorize a release. The prior source-based Composition evidence is
retained in the
[prereplacement snapshot](history/composition-prereplacement-acceptance.md) and
does not make that grammar executable.

The current document contract is in the
[Composition contract](../contracts/composition.md),
[ADR-024](../contracts/adr-024-composition-sql-first.md), and
[ADR-025](../contracts/adr-025-composition-query-variable-graph.md). The primary
requirements are `REQ-COMP-009` and the shared SQL Workspace requirements; their
Mitase bindings locate the code and verification targets.

## Acceptance matrix

| Boundary                       | Required evidence                                                                                                                                                                                                                                                                                             | Candidate evidence state                                                                                                                                                                                                                                                                                 |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Typed graph and strict grammar | Domain, WASM, API, CLI, and Browser accept one typed `spec.variables` graph. Old Composition `sources`, component source references, aliases, and migration paths are absent; invalid old bytes remain available only through raw Entry history and export.                                                   | Strict graph rejection remains shared across adapters. The frontend suite passes 1,380 tests with the portable protocol coverage gate; native/WASM parity fixtures pass locally. Required CI remains pending.                                                                                            |
| Query identity and arguments   | Inline SQL uses server-derived Form bindings and schema descriptors. Saved SQL pins an exact Entry and revision. Each SQL placeholder is explicitly bound to a scalar graph value and checked against the pinned parameter type.                                                                              | All 16 native preview tests pass, including Form rename plus same-name replacement and pinned Saved SQL scalar-copy projection. The API forwards authorized portable SQL without adding authority. Required CI remains pending.                                                                          |
| Evaluation and rendering       | Evaluation uses an immutable Composition, Publication, authorization, and parameter snapshot. It validates each result against the declared descriptor, shares results for repeated references within one snapshot, and leaves preview values in Work. Scalar results require exactly one row and one column. | Core and storage tests cover bounded dependency evaluation, scalar cardinality, typed results, and page continuations. Browser journeys cover stale response ordering, Space changes, source ACL revocation, and same-browser principal changes; CI remains pending.                                     |
| Design bindings                | Table and Metric use the same Variables / Forms / Saved SQL picker. Form and Saved SQL choices normalize to a query variable; the placed component stores only its variable reference. Insert and reference change are atomic and do not open another inspector.                                              | The graph canvas and shared binding picker are restored on Browser routes. Component tests and local Browser journeys verify Form-derived queries, pinned Saved SQL, explicit argument binding, and variable-backed displays; required CI remains pending.                                               |
| Variable workspace             | Design and Variables are the only modes. Inputs, constants, and query variables each have one editor. Query variables reuse the shared SQL Workspace and its diagnostics.                                                                                                                                     | Query variables use the shared SQL Document workspace. Local tests verify reactive SQL edits, explicit count and page controls, pinned read-only source and independent inline copies, and state cancellation across Space changes. Manual assistive-technology and iOS Safari checks remain unrecorded. |
| Shared SQL Workspace           | `/sql/new`, Saved SQL editing, and Composition query variables share the SQL Document editor, formatter, typed operations, and diagnostics. Every executor-admitted read-only construct has a GUI operation and round-trip evidence; unsupported constructs fail closed.                                      | The merged SQL Document workspace is shared by standalone SQL and Composition. Complete construct authoring remains unverified under #4565; the live SQL capability matrix retains its bounded evidence and gaps.                                                                                        |
| Knowledge and recovery         | Save appends an Entry revision. Exact-revision inspection and restore keep their existing meaning. Invalid or old Composition bytes can be inspected as raw history but cannot be silently converted or executed.                                                                                             | The candidate retains raw history and append-only Entry handling and adds strict old-grammar rejection. Required cross-surface CI and raw-history journeys remain to be confirmed against the final candidate.                                                                                           |

## Required journey evidence

Required CI must confirm strict Domain and WASM parity, query type inference,
Saved SQL revision pinning, ACL rechecks, stateless pagination, API and CLI
behavior, and the Browser journeys listed above. Focused local tests are
diagnostic evidence only and do not replace checks on the exact candidate
commit.

Before promoting the complete SQL GUI claim, the SQL capability matrix must show
zero difference between the executor's admitted read-only grammar and the
constructs the shared editor can inspect, modify, and round-trip. This includes
the complete construct-authoring evidence tracked by #4565. General SQL dialects
and unapproved functions are outside that matrix.

## Explicit evidence gaps

The candidate still needs required CI and independent review. Manual
assistive-technology and iOS Safari evidence remains open. The SQL editor
capability matrix and complete construct-authoring evidence under #4565 must be
resolved before declaring full executor-to-GUI coverage complete. No release has
been performed.
