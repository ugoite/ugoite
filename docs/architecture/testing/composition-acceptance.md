---
title: "SQL-first Composition acceptance plan"
---

This is the acceptance matrix for the SQL-first Composition v1 prerelease
candidate. It records implementation and validation evidence without implying
a product release. The prior implementation evidence is preserved in the
[prereplacement snapshot](history/composition-prereplacement-acceptance.md)
and does not apply to the replacement. The accepted next target in ADR-025 is
not covered by this current-candidate matrix and remains planned work.

The normative document shape is in the [Composition contract](../contracts/composition.md)
and [ADR-024](../contracts/adr-024-composition-sql-first.md); the future
variable-graph contract is [ADR-025](../contracts/adr-025-composition-query-variable-graph.md).
The canonical
Composition and workspace requirements are `REQ-COMP-008`, `REQ-FE-079`, and
`REQ-FE-080`; their Feature bindings locate the implementation and its
verification evidence.

## Acceptance matrix

| Boundary | Required evidence | Current evidence state |
| --- | --- | --- |
| Canonical document | Native and WASM parsing agree on one strict v1 envelope, canonical YAML, fingerprint, and diagnostics. `inline_sql` and `saved_sql` are the only source variants. Unknown old `entry_query`, root `tags`, and Composition-owned `label` fields are rejected. | Implemented; Mitase verification passes locally. Native/WASM CI evidence is tracked against the candidate PR commit. |
| Inline SQL identity | Lint/save derives relation-to-Form-ID bindings on the server, persists the ordered result descriptor and fingerprint, and never trusts client-supplied Form bindings. A Form rename keeps the exact bound ID; deletion or loss of access fails closed. | Implemented. The 10-test Composition E2E lane exercises inline SQL-backed Table/Metric results; Form rename/deletion and ACL-denial journeys remain pending. |
| Saved SQL identity | A Composition pins one exact Saved SQL Entry and revision, carries typed parameter mappings and the expected result descriptor, and never follows latest or copies the query body. Typed Saved SQL defaults remain Saved SQL authoring data; Composition defaults are copied only by an explicit user action. | Implemented; exact-revision pinning, explicit Composition defaults, save/reopen, and history restore pass in the local Studio and Metric E2E lane (10/10). |
| Query and Metric behavior | Tables and Metrics share one resolved SQL source result. Metrics use one declared scalar column and distinguish zero rows, one-row `NULL`, numeric zero, multiple rows, type mismatch, and access denial without browser aggregation or an implicit `LIMIT 1`. Existing SQL paging, count, cancellation, resource bounds, and current authorization remain in force. | Implemented; local Composition Metric E2E passes 10/10, including null/empty/zero/multiple-row states and the 100-row Table paging boundary. Authorization and cancellation lanes remain separate CI evidence. |
| Knowledge and recovery | Save creates an append-only Entry revision with the existing receipt and stale-write behavior. Resolve selects the exact Composition revision. Unsupported or invalid stored bytes remain available through raw inspection, export, and history; those paths do not make a prereplacement document executable. Runtime input, result, paging, and drawer state remain transient Work. | Save, exact-revision view, append-only edit, history, and restore pass in the local Studio E2E. Receipt/stale-write and raw invalid-byte recovery still require their focused CI journeys. |
| Shared SQL Workspace | `/sql/new`, Saved SQL detail/edit, and inline Composition SQL use the shared workspace contract. Structured actions round-trip only supported syntax, direct SQL remains authoritative, and destructive structured rewrites require explicit diff confirmation. Preview count and paging are explicit. | Implemented; Deno type checks pass. The current local Composition E2E covers Saved SQL run and save-as-tool, while direct SQL structured-action and explicit diff journeys await CI evidence. |
| Composition Studio | Form selection creates editable inline SQL such as `SELECT * FROM "G"` without inserting `LIMIT 50`. The desktop source rail and narrow-screen drawer support keyboard operation. Design components use semantic values without editable labels or tags. Steady state relies on structure, names, and accessible icon actions rather than explanatory copy or decorative cards. | Local Studio journeys pass 3/3 within the 10/10 Composition E2E lane, including the 390px Inspector flow and 320/375/390px overflow assertions. Manual assistive-technology and Safari checks remain open. |

## Required journey evidence

For the current candidate, required CI must still confirm strict replacement
parsing and native/WASM parity, relation-binding and descriptor drift,
authorization changes, bounded execution, and existing API/CLI behavior. The
local Browser lane passed 10/10 and covers exact Saved SQL revision pinning,
scalar cardinality, typed Input state, save/reopen, history restore, responsive
drawer behavior, and Table paging. Direct SQL structured-action gating,
stale-write/receipt recovery, raw invalid-byte recovery, and cross-surface CLI
use remain separate evidence items.

The final cross-surface lane must exercise the same Space-owned Composition
through CLI and Browser, including raw recovery after an invalid or
prereplacement document is encountered. Existing Saved SQL, Form, Entry,
general SQL, API, and authorization behavior must retain its established
meaning. Required CI and each journey result must be recorded against the exact
candidate commit before the corresponding acceptance claim is promoted.

## Explicit evidence gaps

CI evidence is tracked against each candidate PR commit. No manual
assistive-technology or Safari result, raw invalid-byte recovery journey, or
cross-surface CLI/Browser replacement run is recorded. The prereplacement
snapshot contains historical observations only and cannot close these gaps.
