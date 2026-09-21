---
title: 'Stateless SQL Query'
---

`sql.query` is the canonical read-only SQL operation:

- `POST /spaces/{space_id}/sql/query`
- `POST /spaces/{space_id}/sql/query/count`

The page request contains `sql`, `parameters`, optional `parameter_types`, and
`limit`. A page response contains `columns`, `rows`, `has_more`, and an opaque
`next` continuation. Count is an explicit operation and is never performed as
part of page retrieval.

The first page captures the current `PublicationRef`. A continuation resolves
that same immutable publication and rechecks current authorization on every
request. It carries SQL and parameter fingerprints, a page offset, and a
tamper-detecting signature; it is not an authorization token. Changing SQL,
parameters, or authorization requires a fresh query.

Only one read-only `SELECT` statement is admitted. DataFusion receives only
authorized Form relations from the checkpoint-pinned execution context. SQL
execution does not create a session, write Knowledge, persist query metadata,
or materialize a temporary relation. Resource guards apply to each request's
rows, memory, bytes, timeout, concurrency, and query complexity; there is no
total-result cardinality ceiling.

Saved SQL remains a separate Knowledge mutation. Saving a SQL statement does
not make its later execution state durable.
