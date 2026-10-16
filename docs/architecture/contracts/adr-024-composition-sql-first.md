---
title: "ADR-024 — SQL-first Composition sources and workspace"
---

**Status: Accepted target for prerelease Composition; implementation pending.**
This decision replaces the unreleased Composition authoring and source grammar.
It does not authorize or schedule a product release or version change.

ADR-023 remains the record of its original typed-variable decision. ADR-024
retains typed variables and direct Input/Text bindings while removing source
and display-label semantics that conflict with SQL-first authoring. This ADR is
the normative source for the replacement v1 grammar. The prereplacement
implementation snapshot is preserved in
[the historical contract](../testing/history/composition-prereplacement-contract.md)
and is not a compatibility promise for the replacement.

## Context

The prerelease Composition implementation has separate EntryQuery and Saved
SQL source paths. The target model uses SQL as the only data calculation
boundary while keeping Composition documents portable, Space-owned Knowledge.
This is a prerelease replacement: old Composition documents remain available
as raw Entry history and export data, but the new canonical reader does not
execute the replaced grammar.

The replacement must not change published Form, Entry, Saved SQL, general SQL,
authorization, history, or publication contracts. In particular, SQL remains
read-only, stateless, bounded, and executed by the existing Rust query path.

## Decision

### 0. Canonical v1 wire shape

Composition v1 keeps `format_version: 1`; it does not introduce a format
version 2 or a second supported grammar. The strict document envelope contains
`format: ugoite.composition`, `format_version`, `kind: dashboard`, the
Composition `name`, and `spec`. `spec` contains `variables`, `sources`,
`components`, and `layout`. The document has no root `tags` field. Unknown
fields are rejected.

The persisted source shapes are:

| Kind | Fields | Meaning |
| --- | --- | --- |
| `inline_sql` | `id`, `name`, `sql`, `form_bindings`, `expected_result`, `schema_fingerprint` | `name` belongs to the SQL object and is independent of its stable `id`. `form_bindings` maps SQL relation names to server-derived Form IDs. |
| `saved_sql` | `id`, `entry_id`, `revision_id`, `variables?`, `expected_result`, `schema_fingerprint` | Pins one exact Saved SQL Entry revision. `variables` maps that revision's parameter names to literal values or Composition variable references. |

Each `expected_result` column contains `name` and a portable `type` from
`string`, `boolean`, `integer`, `float`, `decimal`, `date`, `timestamp`, and
`json`. The lowercase SHA-256 `schema_fingerprint` is computed from the
server-confirmed ordered result descriptor. An editor draft may temporarily
carry the exact empty descriptor tuple while it is being resolved; a saved
Composition requires the server-derived descriptor.

The wire structure is illustrated below; IDs and fingerprints are
placeholders, not reusable canonical values:

```yaml
format: ugoite.composition
format_version: 1
kind: dashboard
name: Sales
spec:
  variables:
    - id: year
      type: integer
      required: true
  sources:
    - kind: inline_sql
      id: monthly_rows
      name: Monthly rows
      sql: SELECT month, total FROM "MonthlySales"
      form_bindings:
        - relation: MonthlySales
          form_id: <server-derived-form-id>
      expected_result:
        - name: month
          type: string
        - name: total
          type: decimal
      schema_fingerprint: <sha256-of-ordered-result-descriptor>
    - kind: saved_sql
      id: annual_total
      entry_id: <saved-sql-entry-id>
      revision_id: <exact-saved-sql-revision-id>
      variables:
        year: {variable: year}
      expected_result:
        - name: total
          type: decimal
      schema_fingerprint: <sha256-of-ordered-result-descriptor>
  components:
    - id: year_input
      kind: input
      variable: year
    - id: monthly_table
      kind: table
      source: monthly_rows
    - id: annual_metric
      kind: metric
      source: annual_total
      column: total
  layout:
    kind: flow
    rows:
      - id: controls
        items:
          - kind: component
            component: year_input
      - id: report
        items:
          - kind: component
            component: monthly_table
          - kind: component
            component: annual_metric
```

`form_bindings` is an ordered list of `{relation, form_id}` records. The
`saved_sql.variables` map keys are exact SQL parameter names and values are
typed literals or `{variable: <id>}` references. A source result descriptor is
an ordered list of unique `{name, type}` columns. Neither source variant has a
second display name or alias; the inline SQL object's `name` is its semantic
name. The document has no `entry_query`, root `tags`, or user-editable `label`
field.

Variables contain `id`, `type`, `required`, optional typed `default`, and an
optional `format` hint (`year-month` for `date`). No variable, source, or
component has a user-editable `label`. `input` contains only its `id` and
variable reference; `text` contains its `id`, literal or string-variable
content, and fixed style; `table` contains its `id` and source reference;
`metric` contains its `id`, source reference, output `column`, and optional
numeric presentation (`unit` and `fraction_digits`). The root Composition
`name`, inline SQL object `name`, SQL output aliases, variable IDs, and Text
content retain their distinct semantic roles.

The `layout` remains the flow layout defined by ADR-019: ordered rows place
each component exactly once. Layout controls presentation order only and does
not define SQL execution order. The full accepted field and validation
contract is maintained by this ADR and the SQL-first Composition Requirement;
examples are not a substitute for that contract.

### 1. One SQL source grammar

Composition source kinds are `inline_sql` and `saved_sql` only. Every source has
a stable Composition-local ID.

- `inline_sql` stores one SQL body and one editable semantic object name in
  the Composition document. The name is the SQL object's name, not a separate
  display label; renaming it does not change the source's stable ID. Selecting
  a Form copies its name as the initial SQL object name and creates editable
  SQL such as `SELECT * FROM "G"`; it does not create an EntryQuery source. If
  the user changes the SQL directly, that SQL body remains authoritative until
  the user explicitly regenerates it through a structured operation.
- `saved_sql` pins an existing Saved SQL Entry and exact revision. It does not
  store a second source name or display alias, copy the SQL body into
  Composition, follow a latest revision, or rebind silently; its referenced
  Saved SQL name or the existing localized untitled representation is shown.
  Editing the pinned query requires an explicit inline copy or an explicit
  update of the Saved SQL Entry.
- `entry_query`, its fields, and its Comp-specific query compiler are removed
  from the replacement grammar. No compatibility alias, fallback, or
  Composition migration is introduced. Old `tags`, EntryQuery fields, and
  `label` fields are unknown to the replacement reader and are rejected; they
  are not accepted and then discarded.

When an inline SQL document is linted or saved, Core parses its SQL with the
shared Rust SQL parser and derives immutable Form ID bindings for relation
references. The client cannot supply trusted bindings. Execution uses the
existing stateless SQL planner with those bindings and reevaluates current ACL
and Publication access. A relation keeps the exact SQL identifier and its
server-derived Form ID binding, so a rename follows that ID without rewriting
SQL or resolving a replacement by its new name. Deletion, replacement, or
revoked access fails closed. The Composition resolver may use an authorized
internal descriptor, but general `sql.query` must not accept client-authored
trusted Form bindings.

Each source carries the server-confirmed expected result shape and schema
fingerprint required by Domain validation and drift diagnostics. The
fingerprint is SHA-256 over the canonical ordered `expected_result` columns.
The wire shape uses the portable result types listed above. If inspection
cannot safely determine a result shape, the source remains invalid with a
repairable diagnostic; the Browser does not infer a schema from an empty
result.

### 2. SQL and variable ownership

Composition variables own Composition defaults and the values entered during
preview/use remain transient Work. Inline SQL binds declared variables through
typed SQL parameters. A Saved SQL source maps parameter names to Composition
variables and requires exact declared type equality; SQL text is never
interpolated or concatenated.

Saved SQL variable definitions gain an optional typed default for standalone
creation and preview. It is additive to the existing Saved SQL schema. A SQL
query request still supplies execution values explicitly through the existing
`parameters` and `parameter_types` fields; query request defaults are not
introduced. If a user elects to seed a Composition default from a Saved SQL
default, the value is copied once and the Composition variable then owns it.

Table and Metric displays consume the shared source result. A Metric selects
one declared scalar column and succeeds only for exactly one row, preserving
the distinctions between no row, NULL, numeric zero, multiple rows, type
mismatch, and access denial. The Browser does not aggregate rows or execute
another query engine. SQL aggregate functions remain SQL semantics.

Only changes to a source or to variables used by its SQL plan invalidate its
Table and Metric results. Layout changes do not change query semantics. Paging,
count, continuation, cancellation, resource limits, authorization checks, and
result typing continue through the existing stateless SQL executor.

### 3. Composition identity and persistence

The replacement document keeps its Space-owned Entry revision, stable object
IDs, typed variable declarations, Text body/style, exact Saved SQL revision,
source result descriptors, layout, append-only history, receipts, and stale
write behavior. It removes Composition tags, source display aliases for Saved
SQL, and user-editable labels on sources, variables, and Table/Metric/Input/Text
components. The inline SQL object's semantic name is editable and independent
of its stable source ID. A Saved SQL source uses the referenced Saved SQL name
or the existing localized untitled representation and has no separately
stored name or alias. Other names are derived from the SQL output alias,
variable ID, or Text body as appropriate. Form field names and accessibility
names remain semantic/accessibility values and are not removed.

No runtime input, preview, page, result, or drawer state is persisted. The
replacement keeps the existing raw inspection, export, and Entry history
recovery paths for unsupported old documents. It does not add a legacy
Composition decoder or migration.

### 4. Shared SQL Workspace and visible interaction

`/sql/new`, Saved SQL detail/edit, and inline Composition SQL use one shared
SQL Workspace over the existing SQL parser, typed query operations, CodeMirror,
and result table. Structured controls are enabled only for syntax the shared
SQL capabilities can safely round-trip. Directly edited SQL remains the source
of truth; unsupported structure stays editable as SQL, and a structured rewrite
requires an explicit diff confirmation.

Columns, filter, sort, structure, and variables are secondary icon actions
with accessible names. The primary Save action keeps its visible text label.
Steady state uses structure and names without explanatory paragraphs or
decorative cards. Desktop uses a plain source rail; narrow viewports use a
keyboard-operable overlay drawer. Only result tables scroll horizontally, and
SQL inputs use at least 16px text to prevent iOS focus zoom. All icon targets
and drawer/dialog behavior follow the repository's current UI accessibility
and interaction requirements.

### 5. Delivery boundary

The implementation is delivered in dependent units: accepted contract and UI
specification; Domain/Core/SQL execution and typed Saved SQL defaults;
shared SQL Workspace and Saved SQL routes; Composition Studio/renderer
replacement; then cross-surface acceptance and documentation. No unit exposes a
new Composition editor that can save a document the canonical reader cannot
resolve. Each unit keeps focused evidence and required CI before a dependent
unit proceeds. This ADR does not prepare or publish a release.

## Open evidence

The wire fields and portable result types are fixed in §0. Browser/server
golden-journey evidence, mobile-width checks, and manual assistive-technology
checks remain unverified until their implementation and validation slices are
complete. This target decision does not claim those checks have run.
