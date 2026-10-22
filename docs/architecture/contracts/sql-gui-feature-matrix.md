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
functions. The aggregate structure editor uses that registry for signatures,
accepts the `MEAN` alias, and round-trips nullary, wildcard, single-column, and
variadic `COUNT` forms. `COUNT()` is canonicalized to `COUNT(*)` by structure
serialization because both forms have the same runtime meaning. Multi-column
COUNT is represented as the original first `column` plus an optional list of
additional columns, leaving the existing single-column and wildcard wire shapes
unchanged. The syntax-tree expression picker can wrap an existing typed
expression in a registered aggregate, including arithmetic expressions beyond
a simple Form-column reference; the executor-authorized fixture exercises this
path.

The structure editor is a separate, narrower contract. Its parser accepts a
bounded subset that it can safely analyze and rewrite; direct SQL editing
remains available for other planner-valid statements. The shared checked
inventory is
[`ugoite-sql-rules.json`](../../../shared/sql/ugoite-sql-rules.json). It maps
backend admission boundaries to the current editor operations and records
constructs that are context supplied, preserved without an editor operation, or
outside structured coverage. Its `keywords` list is not consumed by the SQL
admission or lint paths and is not a closed grammar definition.

The matrix pins the DataFusion version that supplies SQL parsing and planning,
along with the shared recursion limit used by saved-query validation and query
execution. The limit is set to 256 so the 32-level boolean editor contract is
accepted consistently at save time and at execution time.
The focused validator compares that pin with `Cargo.lock`, so changing the
runtime parser/planner version requires an explicit matrix review. The construct
inventory is the current reviewed boundary: each admitted SQL category has a
declared editor coverage state, including explicit gaps. This check does not
claim that every DataFusion syntax form is accepted or that bounded editor
coverage is complete.

Both the SQL Document parser and the bounded structure analyzer use
`GenericDialect`, matching DataFusion's default `DFParser` dialect. The checked
matrix and feature-matrix test keep these editor entry points aligned; planning
and authorized execution remain the final authority for whether a query is
admitted.

The typed document contract in the shared matrix pins the SQL Document contract
version, serialized `SqlDocumentNodeKind` set, and `SqlDocumentNodeData`
semantic kind set. Parser-derived semantic data describes syntax only; it does
not infer SQL types, authorize a relation, or approve a query for execution. A
source-backed `unmodeled_expression` keeps parser variants explicit without
claiming they have a structured editor operation. Every construct the read-only
gate delegates to DataFusion planning must name the document nodes that retain
its syntax; constructs rejected by the statement gate must be marked explicitly
as rejected. The focused matrix test compares the matrix against the Rust enums,
the TypeScript wire unions, the parser engine/dialect, and the document contract
version. Adding or changing a runtime-admitted SQL construct therefore requires
a reviewed matrix mapping and a typed document representation in both adapters.
This is a grammar-change review gate, not a claim that every mapped construct
already has a structured GUI edit operation; `editor_coverage` continues to
record that separate capability.

A single trusted Form relation may use a plain table alias; structure edits and
Form changes preserve that alias. The editor also supports one bounded same-Form
self-join with an explicit `INNER`, `LEFT`, `RIGHT`, or `FULL` join-kind choice.
Both tables need distinct aliases, and ON must place the left-alias column
before `=` and the right-alias column after it, with matching supported scalar
types. This path is limited to wildcard projection. Editing the join kind
changes only the operator words and preserves intervening comments and
whitespace; editing ON preserves SQL outside the ON expression. Other clauses
that would make unqualified columns ambiguous remain unavailable in the joined
query. Filter and sort edits fail closed when comments anywhere in the query
could be lost by AST serialization. CROSS and NATURAL joins, USING, multiple
joins, derived relations, reversed equality operands, and other ON expressions
remain outside structured coverage and fail closed. Runtime admission and
authorization remain the existing planner's decision.

The SQL syntax tree editor can change the operator and default/ALL/DISTINCT
quantifier on an existing SELECT set-operation node. A query node can append a
typed SELECT seed with a selected operator and quantifier. Either operand can be
explicitly removed while retaining the other; removal fails closed if a comment
outside the retained operand would be dropped. Add/remove operations return
guarded undo edits. A typed swap operation reorders the exact operand source
ranges while retaining the operator and interstitial trivia. Arbitrary operand
replacement is not yet covered, and BY NAME variants remain read-only. The
feature matrix marks this as bounded coverage; it does not imply complete
set-operation construction.

The SQL syntax-tree editor exposes typed target changes for `CAST`, `TRY_CAST`,
`SAFE_CAST`, postfix `::`, `CONVERT`, and `TRY_CONVERT` nodes from its portable
type list. Rust replaces only the target type, preserving the original cast
form, operand, and surrounding SQL. It reparses the result and returns an undo
transaction. Cast wrappers with internal comments remain read-only where the
target range cannot be established safely. The editor can also wrap a
supported existing expression node in a new `CAST`. Wrapper creation rejects
comment-bearing, unmodeled, and subquery spans; authorized planning validates
conversion and result types.
For a selected projection expression, the inspector can also build a registered
`COUNT`, `SUM`, `AVG`, `MIN`, or `MAX` call and select `DISTINCT` for its one
argument. Rust renders the function from the shared aggregate registry, and
authorized planning checks the expression's input type.
The inspector can wrap a selected supported expression in a searched CASE
seed (`CASE WHEN TRUE THEN <expression> ELSE NULL END`). Its condition and
branches are then edited as typed nodes in the same document. Comment-bearing,
unmodeled, and subquery spans fail closed, and authorized planning checks the
resulting branch types.
Cast target edits preserve `TRY_CAST`, `SAFE_CAST`, double-colon, `CONVERT`, and
`TRY_CONVERT` conversion behavior by replacing only the selected type. Cast
expressions containing internal comments remain read-only where a safe target
range cannot be identified. CASE branch editing remains bounded;
general predicate construction and relational subquery construction remain
explicit editor gaps. The tree editor can add or remove a CASE `WHEN` branch and toggle `ELSE`
through typed Rust operations. New searched branches start as
`WHEN TRUE THEN NULL`; simple CASE branches start with `WHEN NULL THEN NULL`.
Their condition and result are then ordinary typed tree nodes that can be edited
independently. Removing a branch removes only that selected final branch, while
source outside it and comments following it remain intact.

Calls to aggregates in the shared Rust registry (`COUNT`, `SUM`, `AVG`, `MIN`,
`MAX`, and the `MEAN` alias) also expose bounded argument editing. A user can
replace an existing argument with another typed expression node in the document;
`COUNT` can append or remove its final argument when the registry accepts the
resulting arity. The inspector does not create arbitrary expressions or function
calls. `FILTER`, `OVER`, ordered argument clauses, and subquery arguments stay
read-only, and the normal planner remains responsible for type-checking the
resulting SQL before save or execution.

A selected projection item can also take another typed expression node from the
same document. Rust replaces only the original expression span, so an existing
alias and the rest of the query stay in place. This does not add a new
projection, and the planner still decides whether the resulting expression is
valid for the selected Form.

The syntax-tree inspector can also compose two typed expression nodes from the
same query scope around a selected SELECT item or expression node with a closed
binary operator. Rust resolves all three nodes by identity, rejects cross-query
operands and partial overlap with the replaced expression span, and reparses the
result. An operand may be the exact selected expression, allowing the existing
expression to be wrapped. The inspector can also replace an existing
expression node with a typed literal or identifier leaf; comment-bearing spans
are rejected. Authorized planning remains responsible for operand type
compatibility and name resolution.

Every parsed expression node also exposes a typed replacement action that can
copy another expression from the same query scope. Rust rejects cross-query
references, invalid node kinds, no-op edits, and target spans containing
comments. The replacement preserves all source outside the selected expression
and remains subject to authorized planning for type and relation validation.

The SELECT-list inspector can append another expression that already exists as a
typed node in the same document. It inserts one new projection item after the
current list and keeps the rest of the source unchanged. It does not synthesize
an expression that is absent from the query tree.
A separate typed action can append `(SELECT 1)` as a scalar subquery projection.
The new query is parsed immediately and appears as a nested node in the same
tree. Rust keeps existing comments and clauses in place; the action does not
author arbitrary SQL text or add a relation to the nested query. Authorized
planning validates the resulting query.
A SELECT node without a WHERE clause can add a `WHERE TRUE` seed before
GROUP BY, HAVING, ORDER BY, or row limits. The resulting predicate is an ordinary
typed expression node for literal replacement and same-query expression
composition; a second WHERE clause is rejected and the insertion is undoable.
Selected typed expressions can also be wrapped in IN, NOT IN, BETWEEN,
NOT BETWEEN, LIKE, NOT LIKE, IS NULL, or IS NOT NULL templates. Rust uses
editable NULL or empty-string literal seeds; comment-bearing spans fail closed,
and authorized planning validates the result types.

The syntax-tree inspector can append an existing typed expression from the same
query scope to `GROUP BY`. When the clause is absent, Rust inserts it before
`HAVING`, `ORDER BY`, or row limits and preserves surrounding source. Existing
`GROUP BY` expressions remain in place; the authorized planner validates whether
the resulting projection and grouping rules are valid.

The syntax-tree inspector can append an existing typed expression from the same
query scope to `ORDER BY` with a selected `ASC`/`DESC` direction and optional
`NULLS FIRST`/`NULLS LAST`. When `ORDER BY` is absent, Rust inserts it before
`LIMIT`/`OFFSET`; duplicate keys are rejected. The source expression is copied
as a typed document node, and the final query remains subject to authorized
planning.

The syntax-tree inspector can remove an existing projection item or move it to
another ordinal. Removal preserves comments outside the removed span and fails
closed if the selected span contains a comment. Reordering is limited to
comment-free SELECT lists; it retains each item source (including aliases) and
reparses before returning the edit. The context-backed Columns action remains
the path for constructing Form-column slots from a wildcard projection.

An existing CTE name can be renamed from its syntax-tree inspector. Rust
resolves each one-part relation reference through the nearest lexical `WITH`
scope and changes only references bound to the selected declaration in one
source-guarded transaction. Nested same-name CTEs shadow the selected
declaration and remain unchanged. The operation rejects same-scope CTE-name
collisions and returns an undo transaction. The tree editor can also append a
named `name AS (SELECT 1)` seed to a query, create a WITH clause when absent, or
remove an unreferenced CTE. Removal fails closed when its source span contains
comments or a relation resolves to that CTE; a reference to a nested same-name
CTE does not block removing an unused outer CTE. Name collisions are checked in
the target query's own `WITH` list, so a separate nested scope can use the same
name. These operations preserve query text outside their source spans. CTE
ordering, relation authoring within a CTE, and full body restructuring remain
outside verified coverage.

The Filter editor can create typed column-to-parameter comparisons and same-type
column-to-column comparisons for the supported comparison operators,
single-value `IN` / `NOT IN`, `BETWEEN` / `NOT BETWEEN` with independent typed
lower and upper parameters, `LIKE` / `ILIKE` parameter patterns, and `IS NULL` /
`IS NOT NULL` predicates. These leaves compose in bounded nested `AND`, `OR`,
and `NOT` trees. The SQL syntax-tree inspector can also toggle explicit negation
on parsed `IN`, `BETWEEN`, `LIKE`, `ILIKE`, `SIMILAR TO`, `RLIKE`, and `IS`
predicates. Rust locates the parser-classified operator and edits only the `NOT`
token boundary, so operands and comments remain source-backed and the operation
has guarded undo. This control does not construct a predicate or replace its
operands.

The typed expression inspector also edits arithmetic unary `+` and `-` signs on
parser-classified unary expressions, retaining the operand and any comments
between the sign and operand. The edit is source-guarded and undoable; it does
not rewrite other unary operators or construct an expression.

For parsed `IN` and `NOT IN` lists, the syntax-tree inspector can append an
editable `NULL` value or remove the final value when another value remains.
Rust locates the list delimiters from parser-classified tokens and retains
source outside the inserted or removed range. The edit fails closed if a
comment inside the list could be displaced, and each operation has guarded
undo. The normal planner checks the resulting comparison types and SQL.

The shared action bar exposes six primary actions: Form, Columns, Filter, Sort,
Structure, and Arguments. Join, aggregate, group-by, HAVING, distinct,
row-limit, and SQL-tree operations are nested under Structure and remain
available as local actions there. This keeps the primary row compact without
disabling the structure entry point when a particular nested operation is
unavailable.

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

The Sort editor also reads and edits explicit `NULLS FIRST` / `NULLS LAST`
placement on supported simple column keys, alongside ASC/DESC. Omitted NULL
placement stays omitted so the engine's dialect default remains explicit in the
SQL rather than being inferred by the browser.

The matrix records the same SELECT-only boundary as the runtime statement gate.
Relation-free scalar queries align with direct SQL lint and the structure
editor. The SQL executor remains responsible for planner, relation, function,
type, authorization, and resource-limit decisions after admission.

Aggregate parity checks bind the Rust registry, structure parser and validator,
Browser function picker, signature/type gates, and authorized execution tests.
`COUNT(DISTINCT *)`, `COUNT()`, `COUNT(*)`, `MEAN`, variadic COUNT over Form
columns, and registered aggregates around typed expression trees have parser and
serializer coverage. The authorized executor fixture verifies
`SUM(DISTINCT field_101 + 0)` through the same Form ACL context.

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
