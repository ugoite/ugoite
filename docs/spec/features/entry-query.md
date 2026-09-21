---
title: 'Canonical EntryQuery'
---

`EntryQuery` is the shared logical contract for Entry browsing, structured
filtering, text search, FormTable reads, row-reference selection, CLI listing,
and MCP Entry reads. It is a domain query, not a SQL string and not a storage
engine plan.

An `EntryQuery` contains a scope, optional text, logical filters, and ordered
sort clauses. Property fields are valid only for a Form-scoped query. All Forms
queries use system fields, Form identity, and bounded Preview; property names
are never unioned across Forms. The storage compiler adds stable hidden
tie-breakers (`form_id` when needed and `entry_id`) after the requested sort.

Projection is separate from query identity. A page request carries an
`EntryProjection`, a bounded per-request limit, and an optional opaque cursor.
Count is a separate request and is never implicitly coupled to page retrieval.
The result contains stable machine identity and revision data plus only the
requested properties or Preview.

The cursor is signed and contains the Space identity, immutable PublicationRef,
query fingerprint, current-authorization fingerprint, typed sort tuple, Form
identity when applicable, and Entry identity. It is a continuation coordinate,
not an authorization token: every continuation rechecks current authorization
before reading the fixed publication. A projection change does not alter the
query fingerprint; a scope, text, filter, or sort change starts a new chain.

Filter operators and field capabilities are derived by Rust from the Form
definition. Frontend and CLI adapters consume the capability descriptor and do
not maintain independent operator or type tables.

The server exposes this contract through `POST
/spaces/{space_id}/entries/query` and the independent count operation at `POST
/spaces/{space_id}/entries/query/count`. A page continuation keeps the
publication selected by the first request; it does not create a durable pin or
write query state into the Space.
