---
title: Composition contract
description: The portable typed graph and execution boundaries for Composition.
---

[ADR-027](adr-027-text-first-composition.md) records the accepted
Text-first design. [REQ-COMP-010](https://github.com/ugoite/ugoite/blob/main/docs/mitase/requirements/composition/text-first-graph.yaml)
is the behavior authority; current requirements and verification bindings are
tracked in [Mitase](https://github.com/ugoite/ugoite/tree/main/docs/mitase/requirements/composition).
REST behavior is implemented by `ugoite-server` and `/openapi.json` defines
its contract. Release status and compatibility follow the
[release-scope page](../release/release-scope.md) and [ADR-027](adr-027-text-first-composition.md).

## Canonical document

A Composition is one Space-owned Knowledge document stored in an Entry revision.
Save and restore use append-only Entry history, receipts, idempotency, and
stale-write conflicts. No separate database or hidden catalog becomes
authoritative.

The executable `.ugcomp.yaml` envelope has `format_version: 1`, one dashboard
name, a typed variable list, components, and a flow layout. Unknown fields and
the unreleased prior Composition shape are rejected. There is no alias or
migration for that prior shape; raw Entry inspection and recovery remain
available.

Each variable has a unique ID and one kind:

| Kind | Stored definition | Runtime value |
| --- | --- | --- |
| `constant` | Typed scalar type and validated value | Stored typed value |
| `input` | Scalar type, requiredness, optional typed default | Transient user value |
| `query` | Inline SQL, explicit argument bindings, server-derived return descriptor | Authorized result for one evaluation snapshot |

Query parameters bind explicitly to scalar graph variables; matching names do
not bind implicitly. Results declare an ordered `table<Row>` or nullable
`scalar<T>`. The server derives Form identity and result schema; the client
cannot supply trusted metadata. A fingerprint covers ordered result columns
and types. Missing or changed descriptors, missing referenced fields, and
revoked access fail closed, while changes to unused Form fields do not
invalidate an unchanged result contract. Stored SQL keeps authored Form names
and bindings; evaluation and Count use the same resolved SQL and stable Form
relation IDs, so renames or same-name replacements cannot retarget a query.
Current Publication and ACL checks still apply to the bound Form ID.

Composition integer tokens use ASCII `[+-]?[0-9]+` with at most 64 bytes,
within the signed 64-bit range. Leading zeros are accepted. YAML and the
evaluation protocol carry integers as numbers. The Browser preserves exact
numeric tokens for known integer fields and rejects numbers whose exact value
cannot be preserved. Opaque JSON retains its own semantics.

## Components and Text

REQ-COMP-010 defines the `Table`, `Text`, and `Input` components. Table displays
its SQL projection in result order, without a saved column projection or
heading override. Rust parses Text templates into literal and
`{{variable_id}}` reference parts; `\{{` is a literal `{{`. Values render as
text nodes, never HTML, Markdown, or expressions. Integers and decimals retain
their exact representation; booleans render as `true` or `false`, and NULL
uses a neutral no-value rendering. Missing or stale values are not shown as
current output. Components are placed once, and flow layout sets presentation
order only. Coordinates, inputs, previews, pages, results, cursors, loading,
and errors are transient Work.

## Query planning and evaluation

Rust Domain and Core own graph validation, templates, canonicalization,
dependencies, planning, and diagnostics. Composition lint rejects
output-projection wildcards without changing ordinary SQL or published Saved
SQL behavior, and reads no Space data. Authorized planning checks current
relation identity, ACL, Publication, parameter types, and output schema without
returning rows; explicit evaluation uses the bounded Rust SQL path. Scalar
results require exactly one row and column; zero rows, NULL, multiple rows,
type drift, and access denial remain distinct. Runtime never selects a first
row or column implicitly.

Each run is bound to one Composition revision, Publication, authorization
state, input set, and argument fingerprint. Studio may reuse unaffected
dependencies as a runtime optimization; the evaluation contract does not
require partial recomputation. Generation and snapshot checks discard stale
responses. Results are checked against their declared descriptors before
rendering, and are never persisted as Knowledge or materialized in a hidden
table. Paging, continuation, cancellation, and resource limits remain
server-owned.

An explicitly selected query requires only inputs in its dependency closure.
Its results, pages, and cursors publish together after validation; full
evaluation validates every input.

The saved-revision Reader displays transient results only under a current
server-issued authority snapshot lease of at most 30 seconds. While the page
remains active and visible, it checks the lease before expiry; an expired or
unavailable check fails closed. Blur, page hiding, or loss of network clears
the results and table pages. Returning to the page evaluates again under the
current revision, Publication, and authorization. The lease bounds detection
for the active, connected Reader and does not push revocation notifications.

## Studio and Reader

Browser authoring and Reader behavior are defined by REQ-FE-083
(`docs/mitase/requirements/frontend/composition-text-first.yaml`). Composition
Studio's visual hierarchy and labels are described in
`docs/spec/ui/components/composition-studio.yaml`. They use the graph and
execution semantics above; this contract does not define the editor's
interaction flow or layout.

## Compatibility and recovery

Compatibility and release status follow the [release-scope page](../release/release-scope.md)
and [ADR-027](adr-027-text-first-composition.md). Records in the replaced shape
remain raw Knowledge but are not executable or restorable as the current
Composition. Raw inspect, exact reads, export, and history remain available;
restore succeeds for current-shape revisions and rejects the replaced shape.

Published Saved SQL API and CLI operations, the `SQL` system Form, Space/Entry/
Form 0.1 semantics, EntryBrowser/FormTable controls, CI lanes, and merge queue
policy are unchanged. Removing a published Saved SQL path requires a separate
breaking-change decision.
