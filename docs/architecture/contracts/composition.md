---
title: Composition contract
description: The portable typed graph and execution boundaries for Composition.
---

**Status:** [ADR-027](./adr-027-text-first-composition.md) is the accepted
Composition model; its implementation and focused verification are in progress.
This document does not authorize release preparation, version changes, or
publication.

[ADR-025](./adr-025-composition-query-variable-graph.md) records retained graph,
authorization, transient-result, and history decisions. [ADR-026](./adr-026-composition-exact-integers-and-table-view.md)
continues to govern exact integer handling; its Table presentation decision is
superseded by ADR-027. [ADR-019](./decisions.md) continues to govern flow
layout, exactly-once component placement, and presentation order. REST behavior
is implemented by `ugoite-server` and `/openapi.json` is its contract source.

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

## Studio and Reader

Studio has two modes, `Design` and `Variables`, and opens in Design. Design
offers Text, Table, and Input placement. Text is edited at its canvas position
with a compact Title, Heading, Body, or Caption control. Variables presents the
SQL source, graph bindings, type, and local diagnostics without a separate
arguments page. Lint, authorized schema check, and explicit execution remain
distinct actions.

If Design needs a missing Table or Input variable, successful creation in
Variables returns to the pending placement. Cancellation or failure leaves no
partial component. Reader uses the shared renderer without authoring modes,
selection controls, or editing actions. UI layout, accessibility, focus, and
responsive behavior follow the UI specifications and policies.

## Compatibility and recovery

The replaced query/component shape has no compatibility reader and is
scheduled for the next minor version under the repository's Knowledge
compatibility rule. An unsupported historical Composition is not silently
migrated or executed. Raw inspect, export, and history remain available through
the general Entry history and recovery paths. Restore is supported when the
source revision has the current Composition shape; the replaced shape cannot
be restored as an executable Composition.

Published Saved SQL API and CLI operations, the `SQL` system Form, Space/Entry/
Form 0.1 semantics, EntryBrowser/FormTable controls, CI lanes, and merge queue
policy are unchanged. Removing a published Saved SQL path requires a separate
breaking-change decision.
