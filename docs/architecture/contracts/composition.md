---
title: Composition contract
description: The portable typed graph and execution boundaries for Composition.
---

**Status:** [ADR-025](./adr-025-composition-query-variable-graph.md) is the
accepted Composition model. The strict graph, reactive evaluation, and Studio
cutover are in progress in the active change. Full SQL executor-to-GUI grammar
coverage remains an open implementation gate and is not claimed here. This
document does not authorize release preparation or publication.

[ADR-024](./adr-024-composition-sql-first.md) records the superseded,
unreleased source-based schema; [ADR-023](./adr-023-composition-reactive-variables.md)
records the preceding prerelease grammar. Neither is a compatibility contract.
[ADR-019](./decisions.md) continues to govern the flow layout AST, exactly-once
component placement, and presentation order. General REST behavior is
implemented by ugoite-server and specified by /openapi.json; this document does
not create a second API contract.

## Canonical document

A Composition is one Space-owned Knowledge document stored in an Entry
revision. The Space remains authoritative. Save and restore use existing
append-only Entry history, receipts, idempotency, and stale-write conflict
contracts; no separate database or hidden catalog becomes authoritative.

The strict .ugcomp.yaml document keeps format_version: 1. Its envelope contains
format, format_version, kind: dashboard, name, and spec. The spec contains one
ordered variables node list, components, and a flow layout. It has no sources
collection, root tags, or user-editable label fields. Unknown and replaced
fields are rejected.

Each variable has a unique ID and exactly one kind:

| Kind | Stored definition | Runtime value |
| --- | --- | --- |
| constant | Typed scalar type and validated constant value | The stored typed value |
| input | Scalar type, requiredness, and optional typed default | A transient user value, read by a placed Input component |
| query | SQL definition, explicit argument map, and table or scalar return descriptor | The authorized result in one immutable evaluation snapshot |

Portable scalar types are string, boolean, integer, float, decimal, date,
timestamp, and json when supported by the query contract. A table descriptor is
an ordered set of typed columns. Scalar descriptors include nullability.
Domain and Core own type inference and validation. Inputs must be placed and
used; constants and query values are not represented as separate sources.

Query variables contain inline SQL or a Saved SQL Entry and exact revision.
SQL parameters map explicitly to Composition variable IDs; matching names do
not create implicit bindings. Initial bindings accept scalar variables only.
Form IDs and result descriptors are derived by the server, never trusted from
client input. Saved SQL never follows a moving latest revision.

Display components render graph values:

- Input references one input variable.
- Text holds literal content or a string scalar reference.
- Table references one table<Row> variable.
- Value references one display-compatible scalar variable.

A component stores its kind, stable component ID, variable reference, and any
component-owned presentation fields. Form IDs, SQL text, Saved SQL revisions,
query arguments, result descriptors, and source IDs do not appear in renderer
bindings.

The flow layout controls presentation order only. Every component is placed
exactly once. Pixel coordinates, runtime input, previews, pages, results,
loading state, errors, and drawer state are transient Work.

## Query resolution and evaluation

Rust Domain and Core are the semantic authority for graph validation,
canonicalization, dependencies, query planning, types, and diagnostics. Core
compiles the graph to a typed dependency DAG, derives Form ID bindings and
result descriptors, and rechecks current ACL and Publication access. A renamed
Form remains bound to its stable ID. Deleted or recreated Forms, changed
schemas, unavailable exact Saved SQL revisions, or revoked permissions fail
closed.

A scalar query succeeds only for exactly one row and one column. Zero rows, one
NULL row, multiple rows, multiple columns, type drift, and access denial are
distinct outcomes. Runtime evaluation never silently selects a first row or
column. A first-item interaction is an explicit deterministic ORDER BY ...
LIMIT 1 query and produces an optional value when there is no row.

Each run uses an immutable Composition revision, Publication reference,
authorization state, input values, SQL revision, and argument fingerprint.
Only affected dependent queries are recomputed. The same query result can be
shared within one snapshot. Generation and fingerprint checks discard stale
responses. Query results are stateless and bounded; they are never persisted as
Knowledge or materialized in a hidden table.

The Composition path reuses the existing Rust SQL executor. SQL query paging,
continuation, count, cancellation, and resource limits remain server-owned.
Client results are checked against their declared descriptor before they are
retained or rendered. The browser does not calculate graph dependencies or
aggregate fetched rows.

## SQL document and Studio boundary

One Rust-owned SQL document contract serves native and WASM callers. The
durable query is one SQL string; GUI JSON and Form selections are not persisted
as parallel query representations. The editor must preserve comments, quoted
identifiers, aliases, literals, and untouched clauses when another subtree is
edited. Text and tree edits commit as one draft revision. Invalid drafts cannot
run or save.

The executor's read-only SQL allowlist is the authority. Parser success alone
does not admit a query. Every admitted SQL construct must have a corresponding
typed GUI operation, diagnostic, and round-trip fixture before complete
coverage can be claimed. The current SQL structure editor still has documented
coverage gaps; ADR-025's feature matrix tracks them. Browser code does not add a
second parser or SQL generator.

Design Table and Value insertion use one picker with Variables, Forms, and
Saved SQL choices. Form and exact-revision Saved SQL choices resolve or exactly
reuse a compatible query variable and insert the component atomically. Form
Value choices require an explicit aggregate and applicable column. Saved SQL
scalar choices validate one-row, one-column cardinality. The persisted
component keeps only its variable reference. Rebinding uses the same picker
without mutating a shared query variable. Successful insertion closes the
picker without opening an Inspector.

## Failure and recovery boundary

The canonical reader accepts only the typed graph schema. It has no legacy
Composition decoder, old-kind alias, fallback, dual read/write, hidden keeper,
or migration. Old Composition bytes remain inspectable and exportable through
the existing general Entry history and raw inspection contracts; those paths
do not make an old document executable.

Unsupported versions, malformed documents, invalid bindings, and unavailable
resources return local diagnostics without silently changing identity. A
missing Composition revision does not fall back to latest, and a Saved SQL
query uses its exact pinned revision. Browser input, preview, paging, result,
cursor, loading, and error state remain transient Work.
