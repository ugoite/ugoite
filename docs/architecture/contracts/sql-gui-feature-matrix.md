---
title: SQL admission and typed editor coverage
---

Ugoite admits exactly one read-only SELECT query, including SELECT bodies in
CTEs, subqueries and set operations. VALUES, TABLE and non-SELECT nested query
bodies are rejected before planning. The executor exposes only authorized Form
relations and the Core-approved aggregate registry. Parser success alone does
not establish execution admission.

## Sources of truth and current scope

The checked inventory is
[`ugoite-sql-rules.json`](../../../shared/sql/ugoite-sql-rules.json).
`feature_matrix.editor` describes the active SQL Document editor; its
`semantic_operations` list is checked against the Rust enum, TypeScript union
and current inspector. `legacy_subset_editor` records the retained public
subset-editor API. That API's native tests still verify its previous bounded
semantics. The Browser toolbar calls the typed document engine and has no subset
analyzer or generator fallback.

Complete constructibility and editing of every executable SQL variant remains
unverified and is tracked by
[#4565](https://github.com/ugoite/ugoite/issues/4565). The requirement is
unchanged. `coverage_complete: false`, bounded construct coverage and explicit
evidence gaps describe the current implementation; they must not be converted to
a completion claim merely because an AST node exists. Parameterized CAST
attributes and CASE operand switching are typed, lossless edits; a focused
simple-CASE edit is exercised through authorized query execution. Boolean NOT
and bitwise/shift operators have edit and WASM parity evidence, and each
authorized result is exercised from its typed edit. Planner validity for type
parameters remains authoritative. A source-backed `unmodeled_expression`
proves source retention, not that the expression's semantic fields are editable.
Projection aliases have a bounded typed authoring operation. Unquoted
identifiers use ASCII lowercase folding, while quoted identifiers preserve
case. Renaming updates same-scope `ORDER BY` alias references atomically,
including unqualified identifier leaves inside `ORDER BY` expressions when no
other same-scope reference makes the binding ambiguous. Qualified source
references remain unchanged. References in other clauses, comment-bearing
`ORDER BY` clauses, and ambiguous same-scope references fail closed. Full alias
resolution coverage and complete constructibility of the admitted SQL grammar
remain unverified.

## Admission stages

| Stage                | Evidence                                                    | Limit                                           |
| -------------------- | ----------------------------------------------------------- | ----------------------------------------------- |
| Source parse         | Domain parser and lossless document                         | Does not authorize or resolve names             |
| Runtime parse        | DataFusion parser                                           | Does not admit non-SELECT statements            |
| Statement admission  | Shared SELECT-only gate visits nested query bodies          | Does not establish types or physical support    |
| Logical plan         | Bound parameters, function policy and authorized relations  | Does not establish physical support             |
| Physical plan        | Executor plan construction and authorized source validation | Does not prove data-dependent execution success |
| Authorized execution | Actual result values and schema in a specified context      | Does not guarantee future data or authorization |

[`sql-admission-corpus.json`](../../../shared/sql/sql-admission-corpus.json)
records separate expectations for these stages. The native runtime test consumes
the corpus, compares plan and page schemas and checks result values. A projected
IN subquery currently has a physical rejection fixture; an IN subquery in a
WHERE clause has a distinct successful fixture. They must not be conflated. The
scalar corpus uses an explicitly empty authorized relation environment;
non-empty Form data, Entry isolation and edit-generated SQL remain covered in
`crates/ugoite-iceberg/tests/authorized_query.rs`.

The separate
[`sql-document-parser-parity.json`](../../../shared/sql/sql-document-parser-parity.json)
corpus establishes parser parity only. Both parser entry points use
GenericDialect and recursion limit 256. The inventory pins the runtime
DataFusion version and the Domain sqlparser version separately, since the
runtime and source parser can depend on different sqlparser versions.

## Structured operations

The active tree exposes typed operations for relations and repeated joins,
projection expressions, output aliases and Form columns, wildcard expansion, predicates,
grouping keys, nested AND/OR filter conditions with column comparisons,
literal or named-parameter values, typed DATE/TIMESTAMP/INTERVAL and hex values,
NULL/TRUE/FALSE/UNKNOWN and DISTINCT FROM comparisons, LIKE/ILIKE/SIMILAR TO
patterns, and explicit source-backed advanced predicate leaves, HAVING, ordering
keys, DISTINCT, LIMIT/OFFSET, aggregate wrappers, registered function arguments,
casts, CASE branches, CTEs, set operations and subquery seeds. Operations address exact parsed node IDs and
expected kinds. They operate only against the guarded source and contract
version, reparse the result and return a guarded inverse transaction.

Scope checks reject inappropriate cross-query expression copies. Edits preserve
source outside their selected ranges; comment-bearing spans fail closed where
safe rewriting is unavailable. CTE renames resolve lexical binding and nested
shadowing. Multi-step typed construction does not itself promise that every
intermediate draft plans successfully; invalid drafts remain editable while Save
and execution admission remain guarded.

The runtime aggregate registry admits COUNT, SUM, AVG, MIN and MAX with MEAN as
an AVG spelling. The typed picker uses canonical registry names; parsing an
existing MEAN expression does not add a distinct runtime aggregate. Registered
aggregate calls can add or remove a typed `FILTER (WHERE TRUE)` seed; the seed
is an ordinary expression node that remains editable through Rust operations.
The tree can also author `GROUP BY ALL`, `ROLLUP`, `CUBE`, and `GROUPING SETS`.
Grouping-set members are explicit typed groups of same-query expression nodes;
the tree editor preserves those boundaries when reopening an existing clause
and can author the empty grouping set `()`.
Ordered aggregate argument clauses remain read-only because they are outside
the enabled aggregate registry. Native Core
tests check the enabled registry against admission; the tools inventory must not
treat an unused descriptor as an enabled function.

The SQL Document tree can add the DataFusion-supported LEFT/RIGHT SEMI and
ANTI joins with typed ON or USING constraints. LIMIT supports a typed literal,
named parameter, or same-query expression. CTE and derived-table output alias
lists can be added or edited; reference renames follow resolved qualifiers and
lexical CTE shadowing, while ambiguous unqualified bindings fail closed. The
focused authorized query tests establish execution for those four join kinds
and LIMIT expression/parameter forms. Alias edits are parser-backed and
source-safe; their verification does not claim executor admission for every
result shape. Directionless SEMI/ANTI spellings are not offered because
DataFusion's SQL planner does not admit them.

The expression inspector can wrap a typed expression in a one-element
`ARRAY[...]` literal, append typed literal elements in order, move a selected
direct element one position up or down, and remove a selected direct element
from an unaliased complete projection item. Moving preserves the exact comma
separator trivia between the two expressions; comment-bearing projections and
unrecognized separators fail closed. Removing the
sole element retains `ARRAY[]`, which the authorized executor returns as a
zero-element list. Rust marks parsed ARRAY nodes with typed `array_literal`
metadata, which the inspector uses to offer append, reorder, and removal actions only for
the matching ARRAY projection or element. Array elements remain normal typed
expressions and can be edited with the existing literal or expression
operations. The collection-access inspector can add escaped named Struct-field
subscripts, one-based indexes, or a
fully bounded one-based slice with positive literal bounds; it also opens and
updates both bounds on an existing terminal slice when both are simple decimal
integer tokens. New slice construction rejects empty, reversed, overflowing,
injected, or comment-bearing operand spans. Existing-bound edits replace only
the bound literals, retaining comments and surrounding SQL. Native, WASM, DOM,
and authorized-executor tests cover constructing and executing
`ARRAY[4, 9]`, editing it to an empty ARRAY, and executing
`(field_101)[1:2]` over a two-element authorized ObjectList. It also constructs
`(field_101[1])['name']`, which returns the authorized Struct's `name` value;
field names containing single quotes are escaped as SQL string literals.
Editing aliased or comment-bearing array spans fails closed. Omitted-bound
slices, dynamic slice bounds, dot-style Struct access, editing array literals whose
parser range is incomplete, and other collection constructors remain gaps
under #4565. Direct bound editing does not yet cover omitted or dynamic bounds,
strides, or a slice followed by another access in the same expression chain.

## Planner diagnostics and persistence

`sql.query.plan` builds and validates an authorized physical plan and returns
only its output schema. It does not execute the physical plan or collect rows.
Metadata I/O and planning work are still possible. A probe test verifies the
absence of physical execution and confirms that the same probe observes real
query execution.

Definition preflight uses draft SQL, declared parameter types and validated
defaults or typed NULLs. It ignores transient preview input. Saved SQL draft
planning uses current Form-name resolution, matching the existing save path that
derives bindings for the new revision. Exact-revision preview continues to use
stored bindings and types; changing SQL or parameter types selects a draft
instead. A new revision conflict is not resolved by reusing an old plan. Plan
success never authorizes a later Save or Run request.

The controller debounces requests and checks both generation and input identity.
Pending, rejected or stale plans cannot enable Save. SQL, parameter definitions,
Space and revision changes invalidate their corresponding plans. Server-side ACL
or publication changes are rechecked on subsequent requests; local `isCurrent`
does not certify that server state has remained unchanged.

## Verification and remaining work

`tools/sql_feature_matrix_test.ts` checks inventory, parser pins, source
mapping, current evidence selectors and required-runner wiring. Those source
checks do not execute Rust or browser behavior. Required Rust tests, real WASM
protocol tests, frontend coverage and browser E2E supply executable evidence
separately. The owning routes share the Workspace. Composition query variables
use the same Workspace and typed operations. The Composition browser journeys
additionally verify graph bindings, exact revision source, reactive results, and
recovery. This integration does not close the broader construct-coverage gap
recorded above.

Retire the subset editor only after public API compatibility and all replacement
evidence, fixtures, tasks and Mitase selectors are reviewed. Keep explicit gaps
for unmodeled variants, unsupported construction and missing interaction
coverage. Mitase validates declared relationships and evidence references;
Ugoite's delivery workflow executes the tests and owns the merge gate.
