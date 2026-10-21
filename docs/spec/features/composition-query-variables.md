---
title: 'Composition query variables'
---

**Status: accepted target; implementation in progress.** This specification records the
unreleased Composition grammar approved by
[ADR-025](../../architecture/contracts/adr-025-composition-query-variable-graph.md).
The current main branch follows ADR-024 while the active change implements the
replacement. This is not a new reader mode or compatibility layer. Its presence
does not authorize release preparation or publication.

## Knowledge model

A Composition stores one ordered `spec.variables` graph. Each variable has one
unique ID and exactly one definition kind:

| Kind | Stored definition | Runtime value |
| --- | --- | --- |
| `constant` | Required declared scalar type and validated constant value | The stored typed value |
| `input` | Declared scalar type, requiredness, and optional typed default | A transient user value; an Input component must reference it |
| `query` | A SQL definition and declared table or scalar result contract | The authorized result of that query in one evaluation snapshot |

The graph has one name space. Duplicate IDs, unresolved dependencies, cycles,
type mismatches, unused SQL arguments, and input variables without a placed
Input component make a document invalid. Table values have an ordered typed
column descriptor; scalar values carry one portable scalar type and
nullability. Domain and Core own type inference and validation.

Display components are references to graph values, not data definitions:

| Component | Valid reference |
| --- | --- |
| `table` | One query variable returning `table<Row>` |
| `metric` | One displayable scalar variable |
| `text` | Literal text or a string scalar variable |
| `input` | One input variable |

A component stores only its component kind, ID, and variable reference, plus
presentation fields owned by that component. Form IDs, SQL text, Saved SQL
revision IDs, argument bindings, result descriptors, aliases, and source IDs
belong to query definitions or server-derived resolution data; they do not
belong to component references.

## SQL query definitions

A query variable owns either inline SQL text or an exact Saved SQL Entry and
revision reference. It also owns a map from SQL parameter names to Composition
variable IDs. SQL parameters and Composition variables are distinct names and
types: matching names never bind implicitly. Initial bindings accept scalar
variables only. A table result cannot be passed as a SQL parameter or
materialized as an implicit intermediate relation.

The server parses and plans the SQL, resolves authorized Form relations,
derives Form-ID bindings and the typed result descriptor, and validates those
values when committing the Composition. Client-supplied bindings and result
descriptors are not authoritative. On evaluation, Core selects the recorded
Composition revision and exact Saved SQL revision, checks the current
Publication and ACL, and fails closed if a bound Form, schema, revision, or
permission is unavailable. A Saved SQL query never silently follows its latest
revision.

Inline SQL remains one durable SQL string. SQL editor state, a parallel GUI AST,
Form selection state, and query results are not stored as a second query
representation. The Rust SQL document and execution contract is specified by
ADR-025 and the shared SQL Workspace contract; the browser does not define a
separate SQL grammar or type system.

## Scalar result and evaluation rules

A scalar query must produce exactly one column and one row. Zero rows, one
`NULL` row, more than one row, more than one column, type drift, and denied
access are distinct outcomes. The runtime never silently chooses a first row
or column. A first-item experience is an explicit deterministic
`ORDER BY ... LIMIT 1` query; no row produces an optional empty value.

Evaluation compiles the typed graph into a dependency DAG and uses an immutable
snapshot containing the Composition revision, Publication, current
authorization, input values, SQL revision, and relevant parameter fingerprint.
Only changed dependencies and their downstream query nodes are invalidated.
Equivalent references share a result within that snapshot. Generation and
fingerprint checks discard stale responses. Query results, inputs, cursors,
loading state, and errors are transient Work; evaluation does not append Entry
history, write result rows to Knowledge, or materialize hidden tables.

## Cutover and existing contracts

The canonical Composition reader accepts only the replacement grammar. It
rejects the prereplacement `spec.sources`, `component.source`, direct Form
display references, labels, tags, and other old shapes. There is no
compatibility reader, old-kind alias, fallback, dual read/write, hidden keeper,
or migration. An old Composition revision remains available through ordinary
Entry raw inspection, history, and export, but those paths do not make it
executable under the replacement grammar.

This Composition-only cutover does not change the existing Space, Form, Entry,
Saved SQL, SQL API, authorization, Publication, or append-only history
contracts. The graph runtime and client integration are implemented in the active
cutover and have focused validation. Complete SQL executor-to-GUI grammar
coverage remains explicitly incomplete until its parser, editor, and CI
evidence gates pass.

See [ADR-025](../../architecture/contracts/adr-025-composition-query-variable-graph.md)
for the SQL editor matrix, Design picker behavior, interaction rules, and
implementation gates. The target's traceable requirement is
`docs/mitase/requirements/composition/query-variable-graph-v2.yaml`.
