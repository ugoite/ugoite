---
title: SQL admission and structure editor coverage
---

The shared read-only SQL gate requires exactly one DataFusion `Query` whose
query bodies are all SELECTs or set operations composed of SELECT bodies.
`SELECT 1`, CTEs with SELECT bodies, nested SELECTs, and SELECT set operations
remain admitted. `VALUES`, `TABLE`, and non-SELECT bodies nested in a CTE,
subquery, or set operation are rejected with `READ_ONLY_SQL_REQUIRED` before
planning or execution. The stateless
`ReadOnlyAggregateV1` policy currently admits `COUNT`, `SUM`, `AVG`, `MIN`, and
`MAX` as canonical aggregate names, with the `MEAN` alias resolving to `AVG`,
and no user SQL scalar or window functions. The current structure editor does
not recognize `MEAN`; the shared matrix records that alias gap until the editor
implements the same registry.

The structure editor is a separate, narrower contract. Its parser accepts a
bounded subset that it can safely analyze and rewrite; direct SQL editing
remains available for other planner-valid statements. The shared checked
inventory is
[`ugoite-sql-rules.json`](../../../shared/sql/ugoite-sql-rules.json). It maps
backend admission boundaries to the current editor operations and records
constructs that are context supplied, preserved without an editor operation, or
outside structured coverage. Its `keywords` list is not consumed by the SQL
admission or lint paths and is not a closed grammar definition.

The matrix records the same SELECT-only boundary as the runtime statement
gate. Relation-free scalar queries align with direct SQL lint and the structure
editor. The SQL executor remains responsible for planner, relation, function,
type, authorization, and resource-limit decisions after admission.

Aggregate parity is checked here only at the canonical function-name level and
the runtime alias map is checked separately. Signatures,
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
