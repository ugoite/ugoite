---
title: "ADR-025 — Composition query variable graph and complete SQL Workspace"
---

**Status: Accepted; graph implementation is in progress.** This decision
replaces the unreleased Composition grammar and authoring model recorded in
[ADR-024](adr-024-composition-sql-first.md). The active change implements the
strict graph, stateless evaluation, and graph-based Studio; complete SQL
document and executor-to-GUI coverage remains a separate open gate. This
decision does not authorize release preparation or publication.

## Context

ADR-024 gives Composition scalar variables and SQL sources separate identities.
Table and Metric components point to source IDs, and the shared SQL structure
editor handles only a subset of SQL that the read-only executor may admit. A
larger editor cannot be made correct by enabling controls: SQL, parameters,
calculated values, and displayed values need one typed semantic model.

The current Composition model has not been released. Its next schema replaces
that model in place. No compatibility reader, alias, fallback, dual-read,
dual-write, hidden field, or migration will be added for the replaced
Composition grammar. Old Composition Entry revisions remain available through
the existing raw inspection, export, and history contracts; those paths do not
make an old document executable.

The change does not alter published Space, Form, Entry, Saved SQL, SQL API, ACL,
Publication, or append-only history contracts. The CLI remains usable without a
server. SQL execution stays within the existing Rust Core and stateless,
bounded, authorized read path.

### Compatibility policy for the unreleased Composition schema

The replaced Composition schema has not shipped and is outside the v0.1
Knowledge compatibility floor. Implementations replace it with the accepted
strict grammar in place. They must not retain a compatibility reader or writer,
legacy kind alias, API/DTO adapter, dual-read or dual-write path, hidden field,
fallback, compatibility fixture, or migration. Tests must not accept old
Composition fixtures as executable documents. Existing raw Entry history,
inspection, and export remain available under their general contracts; they do
not resolve or execute an old Composition. This policy does not relax
compatibility for the released Space, Form, Entry, Saved SQL, SQL API, ACL,
Publication, or append-only history contracts.

## Decision

### 1. One typed variable graph

`spec.variables` is the only list of values and calculations. Every variable is
exactly one of:

- `constant`: a typed, validated value stored with the Composition;
- `input`: a typed value supplied as transient Work and used by a placed Input
  component;
- `query`: a typed SQL result, either `table<Row>` or `scalar<T>`.

Query variants are inline SQL and an exact Saved SQL Entry/revision. Inline SQL
text is the durable query definition. Saved SQL stores the Entry ID and exact
revision ID, never a moving "latest" reference. The server derives Form bindings
and the result descriptor on commit; client-provided derived metadata is not
trusted.

Each query declares an explicit map from SQL parameter name to Composition
variable ID. SQL parameters and Composition variables are distinct concepts;
matching names never bind implicitly. Initial query arguments accept scalar
variables only. Passing a table result as a SQL parameter or secretly
materializing an intermediate result is outside this contract.

Table, Metric, Text, and Input are renderers of typed values. A Table stores
only a reference to a `table<Row>` variable. A Metric stores only a reference to
a compatible scalar variable. Text stores literal content or a string scalar
reference. Input stores only an input-variable reference. Form IDs, Saved SQL
revision IDs, SQL text, query arguments, labels, and result schemas do not
appear in renderer bindings. Domain validation rejects incompatible types,
missing references, duplicate names, cycles, unused or unplaced inputs, and
invalid query arguments.

The scalar execution contract distinguishes zero rows, one NULL row, multiple
rows, multiple columns, type drift, and access denial. It never silently picks
the first row or column. A "first item" interaction is an explicit deterministic
`ORDER BY ... LIMIT 1` query and returns an optional scalar when there is no
row.

### 2. SQL document and execution contract

One Rust-owned SQL document parser, typed tree, diagnostics, compiler, and
serializer serve native and WASM callers. The durable query is one SQL string;
GUI JSON and Form selections are not stored as parallel query representations.
The editor's in-memory document retains the syntax and trivia needed to keep
comments, quoted names, aliases, literals, and untouched clauses when a
different subtree changes. Text edits and tree edits commit as one document
revision. Invalid drafts stay local to Work and cannot run or save.

The SQL executor remains the semantic authority. A parser or GUI capability
alone does not admit a query for execution. One versioned Rust contract and
feature matrix must tie the query parser, resolver, planner, function and type
allowlists, serializer, and GUI operations together. Any new executable SQL
construct must add its planner, authorization, type, GUI, and regression
coverage in the same change. Browser code does not add a second SQL parser or
generator.

The following matrix records the required editor surface. "Current gap" is an
implementation finding, not permission to narrow the SQL requirement. Runtime
admission for constructs that lack a focused executor fixture remains an
explicit evidence gap until a fixture proves it.

| SQL construct                                                                                                     | Required GUI operation                                          | Current evidence / gap                                                                                                                                                                                              |
| ----------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| One read-only `SELECT`; no multi-statement or write statement                                                     | Query tree, diagnostics, SQL preview                            | The existing SQL API contract requires read-only SQL; unsupported statements fail closed.                                                                                                                           |
| `SELECT` without `FROM`; wildcard, columns, aliases, expressions, `DISTINCT`                                      | Columns and expression tree                                     | Executor fixture `read_only_aggregate_policy_registers_only_aggregate_functions` plans `SELECT COUNT(*)` without `FROM`; broad no-`FROM` expressions and GUI coverage are gaps.                                     |
| `FROM` Form relations, aliases, joins, derived relations, CTEs                                                    | Form/relation tree and join editor                              | Multi-Form authorization is implemented in the SQL path; GUI parsing is a subset. Each construct needs executor and GUI fixtures.                                                                                   |
| `UNION` and other set operations                                                                                  | Structure relation tree                                         | GUI structure currently rejects set operations; executor admission must be established per operation with fixtures.                                                                                                 |
| `WHERE`: nested `AND`/`OR`/`NOT`, comparisons, column references, null tests, `IN`, `BETWEEN`, `LIKE`, parameters | Filter expression tree and argument binding                     | GUI currently accepts only simple parameter comparisons and null checks. Extended operators need execution fixtures and typed GUI controls.                                                                         |
| `GROUP BY` and `HAVING`; `COUNT`, `SUM`, `AVG`, `MIN`, `MAX`, including supported distinct forms                  | Aggregate, group, and condition editors                         | Core's read-only function policy currently admits those aggregates only; GUI supports a narrower subset.                                                                                                            |
| `ORDER BY`: multiple keys, expressions, direction, null placement                                                 | Sort tree                                                       | GUI currently edits Form columns only; expression and null-placement coverage is a gap.                                                                                                                             |
| SQL `LIMIT` and `OFFSET`                                                                                          | Structure row-count controls, separate from result-page size    | The SQL request page limit is stateless and distinct; direct SQL clause editing is not represented by the structure toolbar.                                                                                        |
| `CAST`, `CASE`, arithmetic, nested expressions and scalar subqueries                                              | Typed expression builder                                        | Direct SQL may be parsed/planned where allowed; the current structure editor does not round-trip these shapes. Add paired parser/planner/GUI fixtures before claiming support.                                      |
| Scalar and window functions                                                                                       | Typed function picker, only when the Rust allowlist admits them | The current read-only SQL function policy admits no scalar or window functions. `FIRST`, `FIRST_VALUE`, and `row_number` are not available.                                                                         |
| SQL parameters                                                                                                    | Add, rename, type, reference, and lint actions                  | Current scalar parameter types are string, boolean, integer, float, date, and timestamp. Decimal/JSON binding is not implemented and remains unavailable until the complete transport/runtime contract is extended. |

Only SQL actually admitted by Ugoite's Rust executor is in the GUI coverage
obligation. DataFusion's broader grammar is not itself an allowlist. SQL that is
not admitted remains invalid and editable as a draft; the editor does not
silently reinterpret it or fall back to another engine.

### 3. Shared editor and Design binding picker

`/sql/new`, Saved SQL edit, and Composition query variables use one SQL
Workspace, including the same six actions: Form/relation, columns, filter, sort,
structure, and arguments. The action row stays one line and scrolls internally
at narrow widths. Parameter values appear only when declared and provided. The
Workspace keeps its SQL preview, result table, and diagnostics in the same order
across hosts.

Design Table and Metric insertion use one picker with Variables, Forms, and
Saved SQL entry points. Variable candidates are filtered by type. Selecting a
Form or Saved SQL creates or exactly reuses a query variable and inserts the
component atomically; Form Metrics require an explicit aggregate and column
choice. Saved SQL is pinned to an exact revision and a scalar result is never
inferred from a multi-row or multi-column result. The persisted component then
contains only `variable`. Changing a component's reference uses the same picker
and does not mutate a shared query variable.

Inserting a component closes the picker and leaves the canvas visible. It does
not open an Inspector automatically. The Inspector owns one variable reference
control. The picker has one modal focus owner, closes on Escape, restores focus,
and uses accessible names and 44px targets.

### 4. Reactive execution and persistence

The Rust Domain compiles variables into a typed dependency DAG. A run uses an
immutable snapshot of Composition revision, Publication reference, current
authorization, input values, SQL revision, and parameter fingerprint. Only
affected dependent queries are invalidated; equal query nodes share a result
within that snapshot. Generation and fingerprint checks discard stale responses.
Every run rechecks current authorization and fails closed for deleted Forms,
changed schemas, unavailable Saved SQL revisions, or revoked access.

Composition Knowledge stores query definitions, typed declarations, bindings,
and layout. Input values, previews, pages, results, cursors, loading state, and
errors remain transient Work. Query results are not written as Entries or
materialized as hidden tables.

### 5. Ugoite interaction and delivery gates

The existing Mitase interaction rules remain normative: structure before
explanation, one owner for each item of information, no decorative cards,
reachable primary actions, full-row selection, accessible icon actions, and 44px
targets. Add explanatory text only for a local ambiguity or error. The GUI must
not make a control unavailable merely because a query has no parameter yet;
argument creation is available from the argument editor and from a filter that
needs one.

Implementation is gated by a complete SQL feature matrix, a lossless Rust SQL
document contract, the shared Workspace, a strict typed Composition schema,
reactive execution, the Design picker, and end-to-end specification evidence. No
intermediate gate may claim the complete grammar is GUI-editable. Each change
runs focused validation, independent review, required CI, and merge queue
validation. The target schema is unreleased and does not need an old Composition
reader or migration.

## Implementation status

The strict typed graph, stateless evaluation, and graph-based Studio cutover
are implemented in the active change. Complete SQL executor-to-GUI coverage
remains open: the shared editor still needs the lossless SQL document contract
and paired fixtures for every admitted construct. Keep that coverage gate
explicit until its parser, editor, and CI evidence are complete. No release
preparation, version update, candidate creation, or publication is included.
