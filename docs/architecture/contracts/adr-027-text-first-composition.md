---
title: "ADR-027 — Text-first Composition graph and Studio"
---

**Status: Accepted and implemented on `main` by PR #4893; compatibility review
closed for v0.2.2 by PR #4908.** This decision replaces the Composition query
and component shape defined by ADR-025. The v0.2.1 release, tagged CLI/API and
OpenAPI contract do not expose Composition as a supported surface. The prior
shape was saveable from prerelease `main`, but was not part of the released
compatibility promise. v0.2.2 may therefore reject that shape without adding a
migration or dual reader. Its stored Entry data remains available through raw
inspect, exact reads, export, and history; it cannot be restored or executed as
a current Composition. This decision does not authorize release preparation,
version changes, or publication.

## Context

The Composition document is Space-owned Knowledge. Its current graph and
stateless evaluation provide the right ownership and security boundaries, but
the authoring model exposes more concepts than the product needs: Saved SQL
query references, Metric components, a second Table column projection, and
multiple SQL editing controls. The v0.2.1 release notes and tagged supported
CLI/API/OpenAPI contract contain no Composition surface. The old shape was
saveable from prerelease `main`; those entries remain raw Space-owned Knowledge
and are not rewritten or deleted, but the current executable reader rejects
them. This reviewed v0.2.2 patch changes that unpublished shape without aliases,
migration, or dual read. Published Saved SQL operations and unrelated
EntryBrowser controls remain established user paths.

## Decisions

### 1. One typed graph with inline SQL queries

`spec.variables` contains typed constants, inputs, and query variables. Query
variables own inline SQL, explicit parameter-to-variable bindings, server
resolved Form bindings and result descriptors, and an ordered result-schema
fingerprint. Their result kind is either `table<Row>` or `scalar<T>`. Query
parameter names do not bind implicitly to graph variable names. A Composition
query cannot reference a Saved SQL Entry or revision.

### 2. Table, Text, and Input are the only components

`Table` references a table-valued query and shows its ordered SQL result
columns. It stores no second column projection or heading list. `Input`
references one input variable; its current value remains transient Work.
`Text` owns one plain `content` template string. Metric components and Table
presentation descriptors are not part of the executable Composition model.

### 3. Rust owns Text template meaning

The Rust Domain parser returns ordered literal and variable-reference parts
for `{{variable_id}}` markers. `\{{` emits a literal `{{`. Missing references,
table variables, and JSON values fail validation. Text renders supported scalar
values as text nodes only; there is no HTML, Markdown execution, formatting
expression, or client-side template parser. Integer and decimal values retain
their exact representation. Boolean values use `true` or `false`; NULL uses a
shared neutral no-value rendering. Missing or stale values do not restore an
older result as current output.

A scalar query requires exactly one output column and exactly one row at run
time. Zero rows, NULL, multiple rows, type drift, and authorization failure
remain distinguishable. Planning one column does not claim that row cardinality
is guaranteed.

### 4. Design and Variables are the authoring modes

Studio opens in Design and offers Text, Table, and Input placement. Text is
edited at its canvas position with a compact Title, Heading, Body, or Caption
control. Variables is a focused list and editor; SQL remains one authored
document with local lint, authorized schema check, and explicit execution. A
missing Table query is created and edited in Variables; after Check confirms
its table schema, Use returns to the original Design position without running
the query. A new Input requested from Design or an SQL marker opens the same
Variables editor for name, type, required, and default before Use commits its
first component; SQL marker creation also commits its explicit binding.
Creation from Variables places the component and stays in Variables.
Cancelling a pending operation restores the previous draft. Readers see the
shared renderer without authoring controls. Ugoite's UI policy governs labels,
layout, focus, keyboard operation, and narrow-screen behavior.

### 5. Persistence and authority are unchanged

Save and restore of current-shape documents use the existing Space-owned Entry
revision, idempotency, receipt, and stale-write conflict contracts. Query planning and execution
recheck current Form identity, ACL, Publication, argument types, and result
schema. Query values, pages, and input values remain transient Work. Unsupported
historical Composition documents are not silently migrated or executed; raw
inspect, export, and history remain available. A historical revision with the
replaced shape cannot be restored as an executable Composition.

Stored inline SQL keeps its authored Form names and bindings. For each
evaluation, the authorized resolver binds those references to stable Form
relation IDs; page execution and the matching Count request use the same
ID-bound SQL. Renaming a bound Form or creating a same-name replacement does
not retarget the saved query. Current publication and ACL checks still decide
whether that Form can be read.

Published Saved SQL API/CLI behavior, the `SQL` system Form, Space/Entry/Form
0.1 semantics, EntryBrowser/FormTable controls, CI lanes, and merge queue policy
are unchanged. Removing a published Saved SQL path requires a separate breaking
change decision.

## Verification obligations

1. Native Rust and WASM agree on canonicalization, rejected old variants, and
   ordered Text template parts.
2. Core, REST, CLI, and Browser use the same graph bindings and server-derived
   schema; no adapter reimplements SQL parsing or query execution.
3. Scalar zero-row, NULL, multi-row, schema drift, exact integer/decimal, ACL,
   stale response, save receipt, conflict, and raw-history cases remain
   covered.
4. Studio tests cover the two modes, three components, direct Text editing,
   variable creation and return to placement, and read-only Reader rendering.
5. Public Saved SQL routes and CLI operations and EntryBrowser/FormTable
   behavior remain available.
