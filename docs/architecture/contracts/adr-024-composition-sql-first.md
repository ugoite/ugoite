---
title: "ADR-024 — SQL-first Composition sources and workspace"
---

**Status: Accepted target for prerelease Composition; implementation pending.**
This decision replaces the unreleased Composition authoring and source grammar.
It does not authorize a product release. The first product version containing
the replacement is v0.3.0 or later; it must not ship in a v0.2.x patch.

ADR-023 remains the record of its original typed-variable decision. ADR-024
retains typed variables and direct Input/Text bindings while removing source
and display-label semantics that conflict with SQL-first authoring. Until the
replacement implementation is merged, the current behavior remains described
by the frozen [Composition v1 contract](composition.md).

## Context

The prerelease Composition implementation has separate EntryQuery and Saved
SQL source paths. The target model uses SQL as the only data calculation
boundary while keeping Composition documents portable, Space-owned Knowledge.
This is a prerelease replacement: old Composition v1 documents remain
inspectable and recoverable through ordinary Entry history and export, but the
new canonical reader does not execute the replaced grammar.

The replacement must not change published Form, Entry, Saved SQL, general SQL,
authorization, history, or publication contracts. In particular, SQL remains
read-only, stateless, bounded, and executed by the existing Rust query path.

## Decision

### 1. One SQL source grammar

Composition source kinds are `inline_sql` and `saved_sql` only. Every source has
a stable Composition-local ID.

- `inline_sql` stores one SQL body and one source name in the Composition
  document. Selecting a Form creates editable SQL such as
  `SELECT * FROM "G"`; it does not create an EntryQuery source. If the user
  changes the SQL directly, that SQL body remains authoritative until the user
  explicitly regenerates it through a structured operation.
- `saved_sql` pins an existing Saved SQL Entry and exact revision. It does not
  copy the SQL body into Composition, follow a latest revision, or rebind
  silently. Editing the pinned query requires an explicit inline copy or an
  explicit update of the Saved SQL Entry.
- `entry_query`, its fields, and its Comp-specific query compiler are removed
  from the replacement grammar. No compatibility alias, fallback, or
  Composition migration is introduced.

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
fingerprint required by Domain validation and drift diagnostics. The wire
shape must use the repository's existing portable result types. If inspection
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
components. Names are derived from the inline SQL source name, referenced Saved
SQL name, SQL output alias, variable ID, or Text body as appropriate. Form
field names and accessibility names remain semantic/accessibility values and
are not removed.

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

The exact portable wire fields for Form bindings and expected result
fingerprints must be selected from the existing Domain and SQL descriptor
types during implementation. Browser/server golden journey evidence,
mobile-width checks, and manual assistive-technology checks remain unverified
until their implementation and validation slices are complete. This target
decision does not claim those checks have run.
