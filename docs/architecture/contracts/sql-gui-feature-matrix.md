---
title: SQL admission and structure editor coverage
---

The shared read-only SQL statement gate requires exactly one DataFusion
`Query` whose query bodies are all SELECTs or set operations composed of SELECT
bodies. The `statement_gate_canaries` in
[`ugoite-sql-rules.json`](../../../shared/sql/ugoite-sql-rules.json) are
executed by the Rust admission test: `SELECT 1`, SELECT-only CTEs, nested
SELECTs, and SELECT set operations pass this shape check; `VALUES`, `TABLE`,
write statements, multiple statements, and non-SELECT bodies nested in a CTE,
subquery, or set operation fail with `READ_ONLY_SQL_REQUIRED` before planning.
Passing this first gate is not a promise that a query can execute. DataFusion
planning, the authorized logical and physical plan validators, relation ACLs,
type checking, function policy, and resource limits still apply.

The stateless `ReadOnlyAggregateV1` policy currently admits `COUNT`, `SUM`,
`AVG`, `MIN`, and `MAX` as canonical aggregate names, with `MEAN` resolving to
`AVG`. It registers no user SQL scalar functions, table functions, or window
functions. An admitted aggregate used with `OVER` can reach planning but its
window plan is rejected by the authorized plan validator. Recursive query
plans and user-authored `UNNEST` are also rejected there. The
`function_policy_canaries` distinguish functions rejected during planning
from a supported aggregate that becomes disallowed when used as a window
function. This matches the runtime's staged checks rather than describing a
single closed SQL grammar.

The aggregate structure editor uses the same function registry for signatures,
accepts the `MEAN` alias, and round-trips nullary, wildcard, single-column, and
variadic `COUNT` forms. `COUNT()` is canonicalized to `COUNT(*)` by structure
serialization because both forms have the same runtime meaning. Multi-column
COUNT is represented as the original first `column` plus an optional list of
additional columns, leaving the existing single-column and wildcard wire shapes
unchanged. Aggregate expression trees that are not simple Form-column
references remain outside structured coverage.

The structure editor is a separate, narrower contract. Its parser accepts a
bounded subset that it can safely analyze and rewrite; direct SQL editing
remains available for other planner-valid statements. The shared checked
inventory is
[`ugoite-sql-rules.json`](../../../shared/sql/ugoite-sql-rules.json). It maps
backend admission boundaries to the current editor operations and records
constructs that are context supplied, preserved without an editor operation, or
outside structured coverage. Its `keywords` list is not consumed by the SQL
admission or lint paths and is not a closed grammar definition.

`SqlDocument` can retain generic source-ranged nodes for clauses and query
branches even when no structured edit operation exists for them. In this matrix,
`editor_coverage` records the ability to inspect and mutate a construct through
the specialized GUI operations; a parsed node alone does not count as clause
editing coverage. When Form-specific structured capabilities are unavailable,
editable SQL toolbar actions fall back to the Rust node tree and guarded
source-range replacement so users can edit an exact fragment. Generic
replacement remains a SQL text edit and does not count as a clause-specific GUI
operation in the matrix.

The matrix pins the DataFusion version that supplies SQL parsing and planning.
The focused validator compares that pin with `Cargo.lock`, so changing the
runtime parser/planner version requires an explicit matrix review. The construct
inventory is the current reviewed boundary: each SQL category has a declared
backend decision stage and editor coverage state, including explicit gaps.
`planner_decides` means the statement-shape gate does not decide the category;
the later planner and authorized-plan checks may still reject a query. The
statement and function canaries make those stages executable contract checks.
The matrix does not claim that every DataFusion syntax form is accepted or that
the current bounded editor coverage is complete.

A single trusted Form relation may use a plain table alias; structure edits and
Form changes preserve that alias. The editor also supports one bounded same-Form
INNER self-join: both tables need distinct aliases, and ON must place the
left-alias column before `=` and the right-alias column after it, with matching
supported scalar types. This path is limited to wildcard projection and
preserves SQL outside the relation or ON expression being edited. Other clauses
that would make unqualified columns ambiguous remain unavailable in the joined
query. Filter and sort edits fail closed when comments anywhere in the query
could be lost by AST serialization. LEFT/RIGHT/FULL/CROSS and NATURAL joins,
USING, multiple joins, derived relations, reversed equality operands, and other
ON expressions remain outside structured coverage and fail closed.

The bounded join is routed through the shared `Structure` toolbar action and
uses the existing group-by icon; it is not a separate plus action. Aggregate,
group-by, and distinct controls remain separately surfaced during this
transition, so the planned compact six-action toolbar is not complete.

Simple Form-column projections expose each output alias in the Columns editor.
The editor assigns a transient slot identity from each projection's ordinal in
the exact SQL snapshot, so repeated source columns can be reordered and aliased
independently. A proposal must identify each retained slot at most once; id-less
rows add new slots. The identity is not persisted beside SQL. The legacy
`set_columns` operation remains fail-closed for repeated sources. Projections
with unsupported expressions or unsafe comment/source spans also remain
unavailable to structured edits.

For unqualified `ORDER BY` identifiers, explicit simple projection aliases take
precedence over same-named Form columns. The editor records the alias and its
typed source column separately, so sorting and direction edits preserve SQL's
output-name binding. If a rewrite would resolve a requested Form column through
a colliding output alias, the operation fails closed.

The matrix records the same SELECT-only boundary as the runtime statement gate.
Relation-free scalar queries align with direct SQL lint and the structure
editor. The SQL executor remains responsible for planner, relation, function,
type, authorization, and resource-limit decisions after admission.

Aggregate parity checks bind the Rust registry, structure parser and validator,
Browser function picker, signature/type gates, and authorized execution tests.
`COUNT(DISTINCT *)`, `COUNT()`, `COUNT(*)`, `MEAN`, and variadic COUNT over Form
columns have parser/serializer coverage. Structured aggregate expressions remain
limited to Form-column identifiers; arbitrary scalar expression arguments are
tracked as a separate coverage gap. The matrix records that limit instead of
claiming complete DataFusion expression coverage.

`backend_admission` identifies where the decision is made:
`planner_decides` defers syntax and type validity to DataFusion,
`function_policy_rejects` identifies functions absent from the configured SQL
function registry, `planner_rejects_unregistered` identifies table functions
that are not registered in the user SQL session, `plan_policy_rejects`
identifies logical-plan forms denied by the authorized plan validator, and
`statement_gate_rejects` identifies query bodies rejected before planning.
Likewise, `editor_coverage: none` describes the structure editor; it does not
assert that the backend rejects that SQL. Changes to the closed function
policy, Domain structure protocol, or frontend toolbar actions are checked
against this inventory by Rust and Deno tests.

Composition remains unreleased. Its accepted replacement grammar is strict and
does not need a compatibility reader/writer, legacy alias or adapter,
dual-read/dual-write path, fallback, compatibility fixture, or migration for the
replaced prerelease schema. Raw Entry history and inspection retain their
general contracts and do not make an old Composition executable. This policy
does not change the v0.1 Space, Form, Entry, Saved SQL, SQL API, ACL,
Publication, or append-only history contracts; see
[ADR-025](adr-025-composition-query-variable-graph.md).
