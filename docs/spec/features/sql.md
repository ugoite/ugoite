---
title: 'Ugoite SQL'
---

Ugoite separates SQL execution from Saved SQL persistence:

1. **Saved SQL** under `/spaces/{space_id}/sql` is versioned Knowledge stored in
   the Space.
2. **SQL Query** under `/spaces/{space_id}/sql/query` is a stateless,
   read-only execution surface. It does not create a session, result object, or
   query metadata in the Space.

The CLI uses `ugoite sql query`, `ugoite sql count`, `ugoite sql saved run`, and
bounded `ugoite sql export`. `sql saved run` selects a current or exact Saved
SQL revision through the existing `sql.query` operation. Export repeatedly
calls that operation and streams complete results as NDJSON; it is CLI
orchestration, not another REST operation. Local Core and remote REST targets
use the same request DTOs and semantics. `ugoite sql lint` is parser-only:
syntax validity does not authorize execution or resolve a Form.

New SQL can refer to authorized Forms by their quoted names. Ordinary SQL
resolves those names in the current read publication. A new Saved SQL revision
stores the server-derived Form IDs behind those names, and execution of that
revision keeps those IDs fixed across renames while rechecking authorization.
The Browser editor offers Form-name completions and displays saved/current
names in Saved SQL diagnostics.

SQL accepts exactly one read-only `SELECT` statement. The authorized query
context exposes only permitted Form relations, columns, and functions, and
executes through the fixed Space publication selected for the request chain.
Core-owned SQL function policies admit scalar, aggregate, and window functions
independently. Stateless page and count inputs share the closed
`ReadOnlyAggregateV1` allowlist: `COUNT`, `SUM`, `AVG`, `MIN`, and `MAX` are
available as aggregate functions, while window functions and unrelated
aggregates remain unavailable. The explicit count operation applies its
trusted row-count expression to the authorized query plan separately from
user SQL. Parameters use DataFusion's native `$name` syntax and are bound as
typed scalars before planning; parameter text is never substituted into SQL.

The permitted aggregate functions are `COUNT`, `SUM`, `AVG`, `MIN`, and `MAX`.
`COUNT(*)` counts rows; `COUNT(expression)` ignores `NULL`; `SUM` and `AVG`
accept numeric values; and `MIN`/`MAX` accept comparable scalar values. `SUM`,
`AVG`, and floating aggregate results use checked accumulation: integer and
decimal overflow, unsupported numeric types, and non-finite floating results
fail the query instead of returning a wrapped, partial, or non-finite value.
`COUNT`, checked `SUM`, and checked `AVG` preserve SQL `DISTINCT` semantics.
Window functions are not part of this read-only aggregate policy.
`sql.query.count` remains the count of rows in the SQL result and is distinct
from a SQL expression such as `SELECT COUNT(*) ...`.

`sql.query` returns one bounded page and an opaque continuation when a
deterministic `ORDER BY` allows pagination. The continuation carries the fixed
publication, SQL and parameter fingerprints, pagination position, and
authorization coordinate. It is client-held state, not an authorization token;
each request rechecks current authorization. `sql.query.count` is explicit and
separate from page retrieval. No total-cardinality ceiling or persistent SQL
execution lifecycle is introduced.

Every new SQL page carries a server-owned `result_schema` in column order,
including for empty and all-`NULL` results. Decimal columns use the logical
type `decimal`; their row values are strings containing the exact base-10
value, with no exponent and with the planned fractional scale preserved. They
are never converted through binary floating point. Integer and float columns
retain their portable types. Numeric overflow and non-finite-result failures
use stable SQL aggregate error codes with safe messages.

SQL results are not automatically materialized as Knowledge. A SELECT cannot
write a publication, derived relation, or execution metadata. Future SQL DML
must enter the existing Entry/Form/Change/Publication mutation boundary rather
than updating Iceberg tables directly.
