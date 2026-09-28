---
title: 'Stateless SQL Query'
---

`sql.query` is the canonical read-only SQL operation:

- `POST /spaces/{space_id}/sql/query`
- `POST /spaces/{space_id}/sql/query/count`

The page request contains `sql`, `parameters`, optional `parameter_types`, and
`limit`. An optional `saved_sql: { id, revision_id }` selects an exact Saved SQL
revision; in that mode the stored SQL text is authoritative and the request's
`sql` may be empty. A nonempty supplied `sql` must match the stored revision.
Count accepts the same optional selector. A page response contains `columns`,
`rows`, `has_more`, and an opaque `next` continuation. Count is an explicit
operation and is never performed as part of page retrieval.

The CLI's bounded `sql export` command repeats the page operation and streams
the complete result as NDJSON under a required row limit. It adds no REST
operation, query session, or saved result; file output is committed only after
the final page succeeds. Stdout may contain a partial stream on failure and
must be accepted only after a zero exit status.

The first page captures the current `PublicationRef`. A continuation resolves
that same immutable publication and rechecks current authorization on every
request. It carries SQL and parameter fingerprints, a page offset, and a
tamper-detecting signature; Saved SQL requests also bind the Saved SQL ID,
revision, and resolved Form-binding fingerprint. It is not an authorization token. The signature
uses `UGOITE_QUERY_CURSOR_SECRET` when configured, otherwise the existing
per-Space integrity key; the storage URI is never used as a secret. Changing
SQL, parameters, or authorization requires a fresh query.

Only one read-only `SELECT` statement is admitted. DataFusion receives only
authorized Form relations from the checkpoint-pinned execution context. SQL
execution does not create a session, write Knowledge, persist query metadata,
or materialize a temporary relation. Resource guards apply to each request's
rows, memory, bytes, timeout, concurrency, and query complexity; there is no
total-result cardinality ceiling.

Saved SQL remains a separate Knowledge mutation. Saving a SQL statement does
not make its later execution state durable. New Saved SQL revisions include
server-derived, integrity-protected Form-ID bindings; each execution still
checks the selected Publication and current Form authorization. Older
unbound revisions may execute stable `form_<UUID>` relations, but quoted Form
names require a bound revision.
