---
title: "SQL-first Composition acceptance plan"
---

This is the planned acceptance matrix for the SQL-first Composition v1 target.
It records what implementation slices must prove; it does not claim that any
target behavior, focused validation, Browser journey, mobile check, or
assistive-technology check has passed. The prior implementation evidence is
preserved in the [prereplacement snapshot](history/composition-prereplacement-acceptance.md)
and is not current acceptance.

The normative document shape is in the [Composition contract](../contracts/composition.md)
and [ADR-024](../contracts/adr-024-composition-sql-first.md). The canonical
Composition and workspace requirements are `REQ-COMP-008`, `REQ-FE-079`, and
`REQ-FE-080`; their Feature bindings locate the planned documents and later
implementation evidence.

## Acceptance matrix

| Boundary | Required evidence | Current evidence state |
| --- | --- | --- |
| Canonical document | Native and WASM parsing agree on one strict v1 envelope, canonical YAML, fingerprint, and diagnostics. `inline_sql` and `saved_sql` are the only source variants. Unknown old `entry_query`, root `tags`, and Composition-owned `label` fields are rejected. | Unverified; implementation is pending. |
| Inline SQL identity | Lint/save derives relation-to-Form-ID bindings on the server, persists the ordered result descriptor and fingerprint, and never trusts client-supplied Form bindings. A Form rename keeps the exact bound ID; deletion or loss of access fails closed. | Unverified; implementation is pending. |
| Saved SQL identity | A Composition pins one exact Saved SQL Entry and revision, carries typed parameter mappings and the expected result descriptor, and never follows latest or copies the query body. Typed Saved SQL defaults remain Saved SQL authoring data; Composition defaults are copied only by an explicit user action. | Unverified; implementation is pending. |
| Query and Metric behavior | Tables and Metrics share one resolved SQL source result. Metrics use one declared scalar column and distinguish zero rows, one-row `NULL`, numeric zero, multiple rows, type mismatch, and access denial without browser aggregation or an implicit `LIMIT 1`. Existing SQL paging, count, cancellation, resource bounds, and current authorization remain in force. | Unverified; implementation is pending. |
| Knowledge and recovery | Save creates an append-only Entry revision with the existing receipt and stale-write behavior. Resolve selects the exact Composition revision. Unsupported or invalid stored bytes remain available through raw inspection, export, and history; those paths do not make a prereplacement document executable. Runtime input, result, paging, and drawer state remain transient Work. | Unverified for the replacement; existing general Entry contracts remain separate. |
| Shared SQL Workspace | `/sql/new`, Saved SQL detail/edit, and inline Composition SQL use the shared workspace contract. Structured actions round-trip only supported syntax, direct SQL remains authoritative, and destructive structured rewrites require explicit diff confirmation. Preview count and paging are explicit. | Unverified; implementation is pending. |
| Composition Studio | Form selection creates editable inline SQL such as `SELECT * FROM "G"` without inserting `LIMIT 50`. The desktop source rail and narrow-screen drawer support keyboard operation. Design components use semantic values without editable labels or tags. Steady state relies on structure, names, and accessible icon actions rather than explanatory copy or decorative cards. | Unverified; implementation is pending. |

## Required journey evidence

After the Domain/Core/API slices land, focused evidence must cover strict
replacement parsing, binding derivation, ordered descriptor and fingerprint
drift, exact Saved SQL revision and typed parameters, scalar cardinality,
authorization changes, and bounded SQL execution. Native and WASM must share
canonical output and diagnostics. A draft that cannot resolve a trusted
descriptor must remain invalid and repairable rather than persisting guessed
schema.

After the workspace and Studio slices land, browser journeys must cover Form to
inline SQL, Saved SQL revision pinning, direct SQL edits and structured action
gating, explicit preview paging/count, save and reopen, stale-write recovery,
history inspection, responsive drawer behavior, and accessible keyboard
interaction. Mobile-width and assistive-technology checks remain explicit
acceptance items; no UI success is inferred from static specifications or
mocked operations.

The final cross-surface lane must exercise the same Space-owned Composition
through CLI and Browser, including raw recovery after an invalid or
prereplacement document is encountered. Existing Saved SQL, Form, Entry,
general SQL, API, and authorization behavior must retain its established
meaning. Required CI and each journey result must be recorded against the exact
candidate commit before the corresponding acceptance claim is promoted.

## Explicit evidence gaps

No replacement Domain/Core/adapter implementation evidence is bound yet. No
SQL-first Browser/server golden journey, mobile-width result, manual
assistive-technology result, or cross-surface replacement run is recorded.
The prereplacement snapshot contains historical observations only and cannot
close any of these gaps.
