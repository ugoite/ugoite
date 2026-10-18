---
title: Composition contract
description: The portable document and semantic boundaries for Composition v1.
---

**Status:** The SQL-first Composition model in
[ADR-024](./adr-024-composition-sql-first.md) is implemented in the current
prerelease candidate. [ADR-025](./adr-025-composition-query-variable-graph.md)
defines its accepted next target; the unified query-variable graph and complete
SQL GUI coverage are not implemented yet. This document describes ADR-024's
current implementation contract until its replacement gates are complete. The
prereplacement implementation is not a compatibility contract, and its
document grammar is not supported by the replacement reader. This status does
not authorize release preparation or publication.

ADR-024 documents the source and presentation schema implemented by the
current prerelease candidate. ADR-025 is the normative target for the next
schema; its graph and editor are not implemented yet. [ADR-023](./adr-023-composition-reactive-variables.md)
records the preceding prerelease grammar, not a compatibility contract.
[ADR-019](./decisions.md) continues to govern the flow `layout` AST,
exactly-once component placement, and presentation order. General REST
behavior remains implemented by `ugoite-server` and specified by
`/openapi.json`; this document does not create a second API contract.

## Canonical document

A Composition is one Space-owned Knowledge document stored in an Entry
revision. The Space remains authoritative. Save and restore use the existing
append-only Entry history, receipt, idempotency, and stale-write conflict
contracts; no separate database or hidden catalog becomes authoritative.

The restricted `.ugcomp.yaml` document keeps `format_version: 1`. Its ordered
envelope is `format: ugoite.composition`, `format_version: 1`,
`kind: dashboard`, Composition `name`, and `spec`. `spec` contains typed
`variables`, SQL `sources`, `components`, and a flow `layout`. It has no root
`tags` field. Unknown fields are rejected by the canonical reader.

The source variants are:

| Variant | Canonical fields | Contract |
| --- | --- | --- |
| `inline_sql` | `id`, `name`, `sql`, `form_bindings`, `expected_result`, `schema_fingerprint` | `id` is stable within the Composition. `name` names the SQL object and may be changed without changing its ID. `form_bindings` maps each SQL relation name to a server-derived Form ID. |
| `saved_sql` | `id`, `entry_id`, `revision_id`, `variables?`, `expected_result`, `schema_fingerprint` | Pins one exact Saved SQL Entry revision. `variables` maps SQL parameter names to a scalar literal or `{variable: <id>}`. SQL text and a second source name are not copied into Composition. |

Each source carries a server-confirmed ordered `expected_result` descriptor.
Each column has a unique `name` and portable `type`: `string`, `boolean`,
`integer`, `float`, `decimal`, `date`, `timestamp`, or `json`. Its lowercase
SHA-256 `schema_fingerprint` covers the canonical ordered descriptor. SQL
types and descriptors are determined by the shared Rust query path; the
Browser does not infer a schema from empty results. A transient SQL draft may
use the empty descriptor tuple while the server plans it, but an incomplete
descriptor is not persisted as a Composition revision.

Variables contain `id`, `type`, `required`, optional typed `default`, and an
optional `format` hint. The supported types are `string`, `boolean`, `integer`,
`float`, `date`, and `timestamp`; `year-month` is the date presentation hint.
Defaults must match the declared type. ADR-023 defines accepted raw Input
syntax, bounds, explicit unset behavior, and typed SQL binding. Composition
values are bound as SQL parameters; SQL text is never interpolated.

The component shapes are deliberately small:

- `input` contains an ID and a reference to one declared variable.
- `text` contains an ID, literal text or a direct reference to a string
  variable, and one fixed typography style.
- `table` contains an ID and a source reference; it displays the source's
  bounded paged result.
- `metric` contains an ID, source reference, result `column`, and optional
  numeric presentation (`unit` and `fraction_digits`). A Metric has no title or
  display label of its own.

The Composition has no user-editable tag or `label` field on a source, variable,
or component. The root Composition `name`, inline SQL object's `name`, SQL
output aliases, variable IDs, Text content, Form field names, and accessibility
names have separate semantic roles and remain available. Numeric presentation
is formatting metadata, not a replacement display label.

`layout` is the ADR-019 flow layout. Row order and item order define
presentation only. Every component has one placement; Inputs reference declared
variables; rows and component IDs are unique. Pixel coordinates, CSS, runtime
input, preview pages, result data, loading state, errors, and drawer state are
Work and are not persisted.

## SQL source resolution and results

When Core lints or saves inline SQL, it parses the SQL and derives the Form IDs
behind its relation names. The client cannot supply trusted bindings. The
server persists the relation-to-Form-ID mapping and result descriptor; resolve
uses the exact bindings and rechecks current ACL and Publication access. A
renamed Form remains the same bound Form. A deleted or replaced Form, changed
descriptor, or revoked access fails closed instead of silently binding a new
Form with the same name. General `sql.query` does not accept client-authored
Form bindings.

A Saved SQL source pins its exact Entry and revision. It never follows the
latest revision, silently rebinds, or stores a copied SQL body. The Saved SQL
entry's existing name, or its existing localized untitled representation, is
used when naming the source in the UI. Editing is an explicit inline copy or
an explicit Saved SQL revision update.

SQL remains the only Composition data calculation boundary. Table and Metric
consume one shared source result. A Metric selects a declared scalar output
column and succeeds only for exactly one row; no rows, one-row `NULL`, numeric
zero, multiple rows, type mismatch, and access denial remain distinct. It does
not select the first row, append `LIMIT 1`, or aggregate fetched pages in the
Browser.

Composition uses the existing read-only stateless SQL query path, including
typed parameters, publication pinning, current authorization, explicit count,
bounded paging, continuations, cancellation, and resource limits. Query page
limits and continuation state stay request state, not SQL text or persisted
Composition fields. Changes to a source or one of its SQL variables invalidate
its dependent Table and Metric results; changes to layout do not alter query
semantics.

Saved SQL variable definitions may have an optional typed default for ordinary
Saved SQL authoring and preview. That default is additive to the Saved SQL
contract. SQL query requests continue to require explicit `parameters` and
`parameter_types`; query request defaults are not introduced. Copying a Saved
SQL default into a Composition variable is an explicit one-time user action,
after which the Composition variable owns its default.

## Parsing, canonicalization, and identity

Rust Domain is authoritative for document parsing, typed values,
canonicalization, fingerprints, and diagnostics. Native and WASM surfaces share
that contract. Adapters do not implement a second Composition parser, resolver,
SQL query engine, or semantic validator.

The strict parser rejects duplicate mapping keys, multiple documents, anchors
and aliases, merge keys, custom tags, implicit timestamp conversion, unknown
fields, and malformed source references. Resource limits remain 65,536 input
bytes, nesting depth 64, 4,096 nodes, 8,192 parser events, 32,768 total scalar
bytes, and 256 items in any one Composition collection. Canonical YAML uses
the typed field order, block collections, two-space indentation, LF line
endings, and no BOM.

The document fingerprint is lowercase hexadecimal SHA-256 over the normalized
Composition value serialized as compact JSON. A SQL source fingerprint is
separate: it hashes the server-confirmed ordered result descriptor. It detects
changes in the result contract; it does not replace the document fingerprint
or exact Saved SQL revision identity.

The replacement reader has one canonical Composition v1 grammar. It does not
read the prereplacement `entry_query` source, root `tags`, or persisted display
labels; it adds no alias, fallback, dual-write, deprecation window, or
Composition migration. This does not affect ordinary Entry history, raw
inspection, export, or append-only restore. Those recovery paths preserve the
stored bytes and do not make a replaced Composition executable.

## Failure and recovery boundary

Unsupported versions and malformed documents return stable Composition
diagnostics. Raw Entry inspection, export, and history do not require successful
Composition parsing, so unsupported content remains inspectable and
recoverable. Broken or unavailable source references do not erase the stored
document. Missing or denied Composition reads preserve the existing generic
error behavior; after a Composition read is authorized, unavailable sources
do not disclose Form or Saved SQL identifiers or metadata.

A Composition resolve selects one exact Composition revision. A missing
revision does not fall back to the latest; a Saved SQL source independently
selects its exact Entry and revision. Current source authorization is checked
by the existing query paths. Browser page, scroll, result, and cache state are
transient Work and never become Space Knowledge.
