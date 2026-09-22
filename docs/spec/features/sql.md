---
title: 'Ugoite SQL'
---

Ugoite separates SQL execution from Saved SQL persistence:

1. **Saved SQL** under `/spaces/{space_id}/sql` is versioned Knowledge stored in
   the Space.
2. **SQL Query** under `/spaces/{space_id}/sql/query` is a stateless,
   read-only execution surface. It does not create a session, result object, or
   query metadata in the Space.

The CLI uses `ugoite sql query` and `ugoite sql count`. Both local Core and
remote REST targets use the same request DTOs and semantics. `ugoite sql lint`
is parser-only: syntax validity does not authorize execution or resolve a Form.

SQL accepts exactly one read-only `SELECT` statement. The authorized query
context exposes only permitted Form relations, columns, and functions, and
executes through the fixed Space publication selected for the request chain.
Parameters use DataFusion's native `$name` syntax and are bound as typed
scalars before planning; parameter text is never substituted into SQL.

`sql.query` returns one bounded page and an opaque continuation when a
deterministic `ORDER BY` allows pagination. The continuation carries the fixed
publication, SQL and parameter fingerprints, pagination position, and
authorization coordinate. It is client-held state, not an authorization token;
each request rechecks current authorization. `sql.query.count` is explicit and
separate from page retrieval. No total-cardinality ceiling or persistent SQL
execution lifecycle is introduced.

SQL results are not automatically materialized as Knowledge. A SELECT cannot
write a publication, derived relation, or execution metadata. Future SQL DML
must enter the existing Entry/Form/Change/Publication mutation boundary rather
than updating Iceberg tables directly.
