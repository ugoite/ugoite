---
title: Composition contract
description: The portable typed graph and execution boundaries for Composition.
---

[ADR-027](adr-027-text-first-composition.md) records the accepted
Text-first design. Current requirements and verification bindings are tracked
in [Mitase](https://github.com/ugoite/ugoite/tree/main/docs/mitase/requirements/composition).
REST behavior is implemented by `ugoite-server` and `/openapi.json` defines
its contract. This is a current-main implementation description; Composition
was not included in the published v0.2.1 release. The
[release-scope page](../release/release-scope.md) defines the Current and Stable
documentation boundary, and the [v0.2.1 release note](../../version/releases/v0.2.1.md)
remains authoritative for that stable release.

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

Each variable has a unique ID and exactly one kind:

| Kind | Stored definition | Runtime value |
| --- | --- | --- |
| `constant` | Typed scalar type and validated value | The stored typed value |
| `input` | Scalar type, requiredness, and optional typed default | A transient user value |
| `query` | Inline SQL, explicit argument bindings, and server-derived return descriptor | The authorized result for one immutable evaluation snapshot |

Query parameters map explicitly to graph variable IDs. Matching names do not
create implicit bindings, and only scalar variables supply SQL parameters. A
query result is either an ordered `table<Row>` descriptor or a nullable
`scalar<T>` descriptor. Form identity and result schema are checked by the
server; the client cannot supply trusted derived metadata. A query schema
fingerprint covers its ordered result columns and types. A changed or missing
result descriptor, missing referenced field, or revoked access fails closed;
changes to unused Form fields do not invalidate an unchanged query result
contract.
Stored SQL retains its authored Form names and bindings. Evaluation and its
Count request use the same resolver-produced SQL with stable Form relation IDs,
so a rename or same-name replacement cannot retarget a saved query; current
Publication and ACL checks still apply to that Form ID.

Composition integer tokens use ASCII `[+-]?[0-9]+` with at most 64 bytes,
within the signed 64-bit range. Leading zeros are accepted. YAML and the
evaluation protocol carry integers as numbers. The Browser preserves exact
numeric tokens for known integer fields and rejects numbers whose exact value
cannot be preserved. Opaque JSON retains its own semantics.

## Components and Text

The only components are:

- `Table`, which references a table-valued query and renders its SQL projection
  in result order;
- `Text`, which owns one plain template string in `content`;
- `Input`, which references an input variable whose current value is Work.

Table has no saved column projection or heading override. SQL output names,
aliases, and order determine displayed columns. Text references displayable
scalar variables with `{{variable_id}}`; `\{{` emits a literal `{{`. Rust
parses and validates the template and returns ordered literal/reference parts.
Missing variables, table values, and JSON values are rejected. Values are
rendered as text nodes only; there is no HTML interpolation, Markdown
evaluation, or template expression language. Integer and decimal values retain
their exact representation. Booleans use `true` or `false`; NULL uses a shared
neutral no-value rendering. Missing or stale values do not restore an earlier
result as current output.

Every component is placed exactly once. The flow layout controls presentation
order only. Pixel coordinates, current input values, previews, pages, results,
cursors, loading state, and errors are transient Work.

## Query planning and evaluation

Rust Domain and Core define graph validation, template parsing, canonicalization,
dependencies, query planning, and diagnostics. The Composition-specific SQL lint
profile rejects output-projection wildcards; ordinary SQL and published Saved
SQL behavior remain unchanged. Lint reads no Space data. Authorized planning
checks current relation identity, ACL, Publication, parameter types, and output
schema without returning rows. Explicit evaluation runs through the existing
bounded Rust SQL path.

A scalar query succeeds only for exactly one row and one column. Zero rows, a
NULL row, multiple rows, type drift, and access denial remain distinguishable.
Planning one output column does not claim that row cardinality is guaranteed.
Runtime never selects a first row or column implicitly.

Each run is bound to one Composition revision, Publication, authorization
state, input set, and argument fingerprint. Only affected dependencies are
recomputed. Generation and snapshot checks discard stale responses. Results
are checked against their declared descriptors before rendering, and are never
persisted as Knowledge or materialized in a hidden table. Paging, continuation,
cancellation, and resource limits remain server-owned.

An explicitly selected query requires only inputs consumed by its dependency
closure. Results, pages, and cursors are published together after the closure
passes validation. A full evaluation still validates all its inputs.

The saved-revision Reader retains transient results while it holds a current
server-issued authority snapshot lease of at most 30 seconds. Returning to
the page requires a fresh check before cached results are shown. A changed
authorization or Publication fingerprint clears retained results. An expired
or unavailable check fails closed. The lease bounds detection for the active,
connected Reader and does not push revocation notifications.

## Studio and Reader

Browser authoring and Reader behavior are defined by REQ-FE-083
(`docs/mitase/requirements/frontend/composition-text-first.yaml`). Composition
Studio's visual hierarchy and labels are described in
`docs/spec/ui/components/composition-studio.yaml`. They use the graph and
execution semantics above; this contract does not define the editor's
interaction flow or layout.

## Compatibility and recovery

The v0.2.1 released CLI, REST/OpenAPI, and release notes do not define a
Composition surface. The replaced shape was saveable on prerelease `main`; its
records remain raw Knowledge but are not executable or restorable as the current
Composition. The current-main implementation targets v0.2.2 as a compatible
patch without migration or dual read; v0.2.2 is not yet a published release.
Raw inspect, exact reads, export, and history remain available. Restore succeeds
for current-shape revisions and rejects the replaced shape.

Published Saved SQL API and CLI operations, the `SQL` system Form, Space/Entry/
Form 0.1 semantics, EntryBrowser/FormTable controls, CI lanes, and merge queue
policy are unchanged. Removing a published Saved SQL path requires a separate
breaking-change decision.
