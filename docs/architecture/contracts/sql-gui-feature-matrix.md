---
title: SQL admission and structure editor coverage
---

The shared read-only SQL gate requires exactly one DataFusion `Query` whose
query bodies are all SELECTs or set operations composed of SELECT bodies.
`SELECT 1`, CTEs with SELECT bodies, nested SELECTs, and SELECT set operations
remain admitted. `VALUES`, `TABLE`, and non-SELECT bodies nested in a CTE,
subquery, or set operation are rejected with `READ_ONLY_SQL_REQUIRED` before
planning or execution. The stateless `ReadOnlyAggregateV1` policy currently
admits `COUNT`, `SUM`, `AVG`, `MIN`, and `MAX` as canonical aggregate names,
with the `MEAN` alias resolving to `AVG`, and no user SQL scalar or window
functions. The current structure editor does not recognize `MEAN`; the shared
matrix records that alias gap until the editor implements the same registry.

The structure editor is a separate, narrower contract. Its parser accepts a
bounded subset that it can safely analyze and rewrite; direct SQL editing
remains available for other planner-valid statements. The shared checked
inventory is
[`ugoite-sql-rules.json`](../../../shared/sql/ugoite-sql-rules.json). It maps
backend admission boundaries to the current editor operations and records
constructs that are context supplied, preserved without an editor operation, or
outside structured coverage. Its `keywords` list is not consumed by the SQL
admission or lint paths and is not a closed grammar definition.

The matrix pins the DataFusion version that supplies SQL parsing and planning.
The focused validator compares that pin with `Cargo.lock`, so changing the
runtime parser/planner version requires an explicit matrix review. The construct
inventory is the current reviewed boundary: each admitted SQL category has a
declared editor coverage state, including explicit gaps. This check does not
claim that every DataFusion syntax form is accepted or that bounded editor
coverage is complete.

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

Simple Form-column projections expose their output aliases in the Columns editor
when each source column occurs once. Alias edits preserve the selected source
column and the quoted or unquoted identifier meaning. A repeated source column
keeps the query analyzable and other actions available, but disables the Columns
rewrite because the current operation identifies columns by source name rather
than projection slot. Projections with unsupported expressions or unsafe
comment/source spans also remain unavailable to that operation.

The matrix records the same SELECT-only boundary as the runtime statement gate.
Relation-free scalar queries align with direct SQL lint and the structure
editor. The SQL executor remains responsible for planner, relation, function,
type, authorization, and resource-limit decisions after admission.

Aggregate parity is checked here only at the canonical function-name level and
the runtime alias map is checked separately. Signatures, type restrictions,
modifier round-trips, and runtime behavior for `COUNT(DISTINCT *)` remain
unverified; the structure parser explicitly rejects that form. These gaps are
tracked by [issue #4381](https://github.com/ugoite/ugoite/issues/4381). The
matrix records them so its name check is not read as complete aggregate parity.

`backend_admission: planner_decides` means the statement gate does not decide
that construct's validity. It does not promise that every use will plan or
execute. Likewise, `editor_coverage: none` describes the structure editor; it
does not assert that the backend rejects the SQL. Changes to the closed function
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
