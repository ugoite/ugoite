---
title: SQL admission and structure editor coverage
---

# SQL admission and structure editor coverage

The current read-only SQL gate requires one DataFusion `Query` statement AST,
but does not restrict the query body's form to `SELECT`. Both `SELECT 1` and
`VALUES (1)` pass this gate. The production stateless query path accepts
`VALUES` through request preparation and logical planning, then rejects it
during authorized physical-plan validation because its plan has no approved data
source. A focused `AuthorizedQueryContext::execute_stateless_page` test checks
this rejection, and a `UgoiteService::query_sql` integration test checks the
public error. The admission gate remains unchanged. The stateless
`ReadOnlyAggregateV1` policy currently admits `COUNT`, `SUM`, `AVG`, `MIN`, and
`MAX` as aggregates, with no user SQL scalar or window functions.

The structure editor is a separate, narrower contract. Its parser accepts a
bounded subset that it can safely analyze and rewrite; direct SQL editing
remains available for other planner-valid statements. The shared checked
inventory is
[`ugoite-sql-rules.json`](../../../shared/sql/ugoite-sql-rules.json). It maps
backend admission boundaries to the current editor operations and records
constructs that are context supplied, preserved without an editor operation, or
outside structured coverage. Its `keywords` list is not consumed by the SQL
admission or lint paths and is not a closed grammar definition.

The matrix records two contract gaps. First, direct SQL lint reports an error
when a `SELECT` has no `FROM`, although `SELECT 1` passes the backend statement
gate; this is tracked by
[issue #4382](https://github.com/ugoite/ugoite/issues/4382). Second, that gate
is weaker than the documented SELECT-only boundary because the query AST body is
not restricted; narrowing it is tracked by
[issue #4383](https://github.com/ugoite/ugoite/issues/4383). `VALUES` is
rejected downstream by physical-plan authorization. This inventory does not
change lint or SQL admission behavior; the SQL Workspace follow-up should
reconcile these differences.

Aggregate parity is checked here only at the function-name level. Signatures,
type restrictions, modifier round-trips, and runtime behavior for
`COUNT(DISTINCT *)` remain unverified; the structure parser explicitly rejects
that form. These gaps are tracked by
[issue #4381](https://github.com/ugoite/ugoite/issues/4381). The matrix records
them so its name check is not read as complete aggregate parity.

`backend_admission: planner_decides` means the statement gate does not decide
that construct's validity. It does not promise that every use will plan or
execute. Likewise, `editor_coverage: none` describes the structure editor; it
does not assert that the backend rejects the SQL. Changes to the closed function
policy, Domain structure protocol, or frontend toolbar actions are checked
against this inventory by Rust and Deno tests.
