---
title: Composition contract
description: The portable document and semantic boundaries for Composition v1.
---

**Status:** frozen as Composition v1 (`implementation_contract_frozen = true`).
The v1 domain model, restricted parser, canonicalization, fingerprinting,
native/WASM parity, Rust Core source resolver, the portable
`composition.save`/`composition.restore` persistence operations, the nine CLI
commands, and the Browser save/list/resolve/query paths have focused
implementation and test evidence. The full end-to-end runtime journey and
release acceptance remain incomplete; see the
[acceptance plan](../testing/composition-acceptance.md). Post-freeze semantic
changes require a v1 compatibility ruling recorded against this contract.
Governance decision ADR-019 re-freezes this contract on the dashboard flow
layout: the pre-release `sections` grammar is replaced by first-class
`layout`, with `text` components and parameter placement added.
`format_version` stays 1 with no migration reader.

Frozen v1 pin (see `composition_v1_freeze_pins_portable_contract` in
`ugoite-domain` and `composition_v1_freeze_pins_operation_inventory` in
`ugoite-api-client`):

| Item | Frozen value |
| ---- | ------------ |
| Envelope | `format: ugoite.composition`, `format_version: 1`, `kind: dashboard` only |
| Parser limits | 64 KiB YAML bytes, nesting depth 64, 256 collection items |
| Tags | document tags are the sole semantic value and equal stored Entry tags |
| Source grammar | `entry_query` (Form scope, six filter operators, two sort directions, fields/preview projection, optional Created/Updated display columns) and exact `saved_sql` (EntryId + RevisionId, ordered unique `expected_result`, eight logical types) |
| Metric identity | exactly one typed `definition` (`aggregate` or `sql_scalar`) with a stable FieldId or exact column name; the unshipped `source` + `value_field` shape is rejected |
| Components | `text` / `metric` / `table`; `flow` layout rows/items, unique row and component IDs, exactly-once component placement, parameter controls for required parameters without defaults, empty layouts and empty rows invalid; text styles `title` / `heading` / `body` / `caption` |
| Fingerprints | document SHA-256 over normalized value; per-source schema fingerprints over FormId + used-field snapshot |
| Diagnostics | 14 caller-visible codes; missing/denied sources conceal as `source_unavailable` |
| Operations | `lint`, `list`, `get`, `history`, `resolve`, `save`, `restore` (export stays a CLI file operation) |
| Idempotency | `Idempotency-Key` required on save/restore; same key + same payload replays, differing payload conflicts |
| Raw recovery | unsupported versions and broken references keep raw inspect, export, and history |
| Save-as-tool scope | Form-scoped EntryQuery views and exact Saved SQL revisions save as tools; All-Forms Search (`/search`, All scope, no `form_id`) is inexpressible in v1 and stays disabled fail-closed rather than approximated |

### Compatibility ruling: optional EntryQuery display timestamps

This pre-release extension adds the optional `display_system_fields` selection
to EntryQuery sources and keeps `format_version` at `1`. Missing selections
default to empty and empty selections are omitted from canonical documents, so
existing saved Composition documents remain readable without migration. The
selection does not change query projection, filtering, sorting, or authorization.

## Portable document

A Composition is a user-owned Knowledge document represented as one Entry
revision in its Space. The Space remains its authority. History, restore,
receipts, and stale-write conflicts use the existing Entry revision contract;
no separate database or hidden catalog becomes authoritative.

The exchange format is a restricted `.ugcomp.yaml` document at format version
`1`. Its envelope is ordered as `format`, `format_version`, `kind`, `name`,
`tags`, and `spec`. `format` is the fixed value `ugoite.composition`, and
`kind` is `dashboard` for v0.2.2. `tags` are the Composition Entry's tags and
are saved and read as the same ordered list as `EntryMetadata.tags`; a save
request has no second tag value. The `spec` contains typed parameters, sources,
components, and a first-class dashboard flow `layout`. A source identifies either an Entry query template
or an exact Saved SQL revision. Components refer to named sources, and layout
rows carry ordered component and parameter-control items that reference them.
Parameters keep their semantic definition under `spec.parameters`; the layout
only places controls bound to parameter IDs. An `entry_query` source carries a sorted `field_schema`
snapshot for the property fields used by its query and typed aggregate
definitions. A preview projection uses all current property fields; an
explicit projection, filter, or sort uses the fields it names. A typed aggregate
binds its `field_id` through `field_schema` and may use a
field that the table projection does not display. Its full matching set is
defined by the source Form, filters, and text search; table page size, current
page, sort, and projection do not change the aggregate set. Execution must
revalidate the field against the current authorized Form schema. When
text search is present, the snapshot also
includes every text-searchable property field under existing EntryQuery
semantics. The text-search expansion excludes `binary`, `list`, `object_list`, and
`asset_reference` fields. A field of those types still appears when the query
otherwise uses it, such as in a projection. Each snapshot row contains
`field_id` and `field_type`, plus `reference_form` for a RowReference or
`items` for a typed List when those type details are present.

The complete illustrative document is maintained in
`crates/ugoite-domain/tests/fixtures/composition/monthly-expense.ugcomp.yaml`.
It is a contract fixture for domain, parser, canonicalization, and WASM parity
tests, not an assertion that persistence or rendering is already shipped.
The additive typed metric grammar is covered by
`typed-metric-definitions.ugcomp.yaml`. An aggregate definition names an
EntryQuery source and one operation (`count_rows`, `count_values`, `sum`,
`avg`, `min`, or `max`). `count_rows` has no `field_id`; `count_values` counts
non-NULL field values, and the other field-based operations use a stable
`FieldId`. `sum` and `avg` require a numeric field; `min` and `max` accept
numeric, date, or timestamp fields, excluding time-only fields. Numeric
formatting is optional display metadata. Date and timestamp `min`/`max` results
omit it and use type-based rendering. Aggregate field identity and type do not depend on the table
projection. A `sql_scalar` definition names an exact Saved SQL source
and output column; its presentation is optional. The typed grammar is parsed,
validated, canonicalized, and fingerprinted through the shared Rust/WASM
contract. The Core resolver validates these typed definitions and binds their
source schema. Aggregate execution and SQL Scalar execution are separate query
operations. The unshipped former `source` plus `value_field` shape is rejected
as `invalid_composition`; no migration reader or conversion is provided. This
does not change ordinary SQL documents, Saved SQL revisions, or Space Knowledge.

The typed v1 model supports `string`, `boolean`, `integer`, `float`, `date`, and
`timestamp` parameters. A parameter may carry a typed default and a display
format hint; `year-month` is a hint and does not evaluate an expression or
change the parameter value. Parameters and `text`/`metric`/`table` components may
also carry optional display `label` text. A label is emitted in canonical YAML
and included in the document fingerprint when present, and omitted when
absent. Labels do not replace stable parameter, component, source, or field IDs
and do not change source or query binding semantics. Source values are literals
or named parameter references. An `entry_query` template scopes property
`FieldId`s to its `FormId`, snapshots query-used field types, and carries the
existing query filters, sort, text, projection, and page-size semantics. Text may be a literal
or parameter reference; a parameter used for text is bound as a string. Its
projection is either preview or an ordered list of property `FieldId`s. The
optional `page_limit` defaults to 100 and is still bounded by the existing
EntryQuery maximum. Optional `display_system_fields` independently selects
Created and Updated table columns and defaults to empty when omitted; these
columns do not widen the EntryQuery projection. A `saved_sql` source carries an
`EntryId`, an exact `RevisionId`, literal-or-parameter variables, and an ordered
`expected_result` descriptor. Each expected result field has a unique non-empty output name and
one portable logical type: `string`, `boolean`, `integer`, `float`, `decimal`,
`date`, `timestamp`, or `json`. Decimal scalar values are JSON strings in
plain notation with no exponent, no leading integer zeros, and no negative
zero; fractional scale, including trailing zeroes, is preserved. The descriptor describes the exact revision's expected
result shape; it does not claim that a SQL backend can statically infer the
shape. Backend SQL type names are not part of the portable contract.
Dashboard components are
`text`, `metric`, or `table`. A `text` component carries inline `text` and one
fixed `style` (`title`, `heading`, `body`, or `caption`); it emits no source
request and its resolve binding carries layout position with no source. `metric` and `table` components refer to
named sources. The flow layout defines component render order by row-array
order and item order. Every row ID and component ID is unique; every component
is placed by exactly one layout item and every placed parameter references a
declared parameter exactly once. Unknown, duplicate, or unreferenced component
references, unknown or duplicate parameter references, empty layouts, empty
rows, and duplicate row IDs make the document invalid, as does a required
parameter without a default and without a layout control. Component
declaration order does not supply a fallback render order. Pixel coordinates,
CSS, and canvas state are never layout content.

An `aggregate` definition names an `entry_query` source and an operation.
`count_rows` has no field; other operations use the source Form's stable
`FieldId`. The resolver checks the operation against the current Form field
type and retains the field in the source-schema fingerprint even when it is
not projected for the table. A `sql_scalar` definition names a `saved_sql`
source and exact output column; that column must uniquely match a declared
scalar `expected_result` column. The two source types cannot be interchanged.

Integer metric values are canonical decimal strings, including values outside
JavaScript's safe integer range; JSON numbers are rejected to prevent Browser
precision loss. Decimal metric values must be plain JSON strings with an optional minus sign,
an integer part, and an optional non-empty fractional part. Exponents, leading
integer zeros, and negative zero are rejected; fractional scale is preserved.
Runtime metric values declared as `date` must be JSON strings containing a
valid Gregorian date in normalized `YYYY-MM-DD` form. `timestamp` values may be
normalized local timestamps at minute precision (`YYYY-MM-DDTHH:MM`) or at
seconds precision with an optional normalized fractional part
(`YYYY-MM-DDTHH:MM:SS[.fraction]`), or valid RFC 3339 timestamps with an
explicit `Z` or numeric `±HH:MM` offset. Fractional seconds have one to nine
digits; normalized local timestamps omit trailing zeroes. The shared domain
validator rejects malformed non-null values with `metric_result_type_mismatch`
and accepts SQL `NULL` for a one-row scalar. Cardinality and exact
selected-column checks belong to the Core SQL Scalar probe; zero rows and
multiple rows use `metric_scalar_expected_one_row`. Metrics are never computed
from a browser or CLI query page. Aggregate execution, SQL Scalar execution,
and their full query journey are tracked as separate runtime work.

The shared Rust and portable API diagnostic vocabulary includes
`unsupported_format_version`, `invalid_composition`, `parameter_unknown`,
`parameter_missing`, `parameter_type_mismatch`, `source_unavailable`,
`missing_field`,
`field_type_changed`, `source_schema_changed`, `metric_result_not_scalar`,
`metric_result_type_mismatch`, `metric_scalar_expected_one_row`,
`metric_result_column_missing`, and `metric_result_column_ambiguous`. Audit note: `missing_form`,
`saved_sql_revision_missing`, and `not_authorized` were removed from the
portable vocabulary as unreachable. The current resolver projects a missing,
denied, or mismatched source descriptor to `source_unavailable`; it does not
fall back from a missing exact Saved SQL revision to latest. A missing or denied
Composition read uses the existing generic 404/error response shape, so its
existence is not disclosed. After Composition access is authorized, source
availability does not disclose Form or Saved SQL identifiers or metadata.
Field-level diagnostics are returned only after the source Form is authorized.
UI messages are supplied by the consuming surface and are not part of this
domain contract.

## Semantic authority

Rust typed domain values are the semantic authority. Native and WASM callers
use the same parser, validator, canonicalizer, fingerprint, and diagnostic code
definitions. Browser TypeScript does not implement a second Composition
parser, resolver, query engine, or semantic validator.

The v1 syntax rejects duplicate mapping keys, multiple documents, anchors and
aliases, merge keys, unsupported/custom tags, implicit timestamp
interpretation, and unknown fields. Date-like plain scalars remain strings;
ambiguous string values that look like booleans or numbers must be quoted.

COMP-011 evaluated `serde-saphyr` 1.3.0 and `yaml-rust2` 0.13.0. Both parsed
the shared fixture and compiled for `wasm32-unknown-unknown`. `yaml-rust2`
exposes useful parser events, but requires a Ugoite-owned layer for typed value
construction, duplicate-key detection, and collection budgets. `serde-saphyr`
was selected because it deserializes directly into the shared Serde model and
exposes explicit duplicate-key, syntax, and resource-budget controls. The
domain parser first performs bounded format-marker and version inspection,
then strictly parses supported v1 input; both passes use `serde-saphyr`.

The parser accepts at most 65,536 input bytes, 64 levels of nesting, 4,096
nodes, 8,192 parser events, 32,768 total scalar bytes, and 256 items in any one
Composition collection. The version probe accepts at most two documents and
limits anchors, aliases, merge keys, and retained anchor data; strict v1 parse
rejects those constructs outright. The shared monthly-expense fixture measured
1,617 input bytes, 159 nodes, 200 events, depth 8, and 921 scalar bytes. The
ignored profile test parsed 1,000 full documents in about 1.25 seconds in a
debug build on the implementation host. Reproduce it with
`mise run profile:composition:parser`. This is a reproducibility sample, not a
performance threshold.

Canonical YAML uses schema field order, block collections, two-space
indentation, LF line endings, and no BOM. Canonical output is derived from the
normalized typed value; comments are not preserved.

Composition has three distinct fingerprint scopes. The document-level semantic
Composition fingerprint is lowercase hexadecimal SHA-256 over the complete
normalized `CompositionDocument` serialized as compact JSON. It represents the
portable document identity, so any semantic change to the envelope or spec
changes this fingerprint while formatting changes do not. It is not a source
schema fingerprint.

Each `entry_query` source has a narrower source-schema fingerprint. The core
resolver computes it from only the source `FormId` and the normalized schema
snapshot for fields that the resolved EntryQuery uses. This includes
projection fields (all property fields for preview), filter and sort fields,
fields searched under existing EntryQuery text-search semantics, and typed
aggregate fields even when they are absent from the table projection. An
aggregate field changes the source-schema fingerprint but does not widen the
table query projection. The retired `source` plus `value_field` shape has no
reader. The
snapshot is unique and normalized by `FieldId`; each field contributes its
logical type, List item metadata, and RowReference target metadata. Unrelated
Form fields, labels, and the rest of the Composition document do not affect
this fingerprint. In particular, text search alone does not make List fields
used because existing EntryQuery semantics exclude them from text-search
expansion. The query-template contribution is implemented in
`compile_entry_query_source` and covered by the focused resolver fingerprint
tests in `crates/ugoite-core/src/composition.rs`.

Each `saved_sql` result fingerprint includes its exact Saved SQL Entry and
Revision, the normalized ordered `expected_result` descriptor, the used
variable schema, and the selected result column when the component is a metric.
Changing the declared output names, logical types, order, or metric selector
changes this fingerprint. Saved SQL revision selection remains exact and does
not fall back to a newer revision.

The shared native/WASM fixture can be executed with
`deno run -A crates/ugoite-wasm/tests/composition_parity.ts`.

## Failure and recovery boundary

Unsupported format versions and invalid documents return stable diagnostic
codes. They do not make raw inspection, export, or revision history unavailable.
The parser identifies the format marker and `format_version` before strict v1
typed deserialization; only a supported v1 document is deserialized as
`CompositionDocument`. Raw
inspection, export, and history use the stored revision independently, so an
unsupported or malformed document remains recoverable. Broken source
references are execution failures; they do not erase the stored document.

Entry query templates resolve to the existing bounded EntryQuery contract.
Composition resolution reads and authorizes the caller-selected Entry ID and
exact Revision ID before parsing. Its typed spec and reported Composition
revision reference come from that same revision; a missing selected revision
does not fall back to latest. Saved SQL references independently identify an
exact Entry and Revision and never fall back to the latest revision. Current
authorization is re-evaluated by the existing query paths.

Browser page, scroll, result, and cache state are transient Work. They are not
fields of a Composition and do not become Space Knowledge.

### Restore retry identity

`restore_composition_local` and
`restore_composition_authorized_for_principals` are one-shot convenience
methods. Each call creates a fresh operation ID; if a response is lost after a
restore commits, calling either convenience method again can append another
restore. A caller that may retry an uncertain outcome uses the corresponding
`_with_operation_id` method and retains the operation ID and publication-
defining values for that logical restore.

Callers assign one operation ID to one logical restore and must not reuse it
for a different restore. Publication identities are scoped by Space; within a
Space, an ID is bound to its Composition Entry, source revision, exact base
revision, and author. Reuse in that Space with different publication-defining
values returns an idempotency conflict. Authorized calls also recheck the
caller's current Entry and Form permissions on every attempt, including a
replay, before resolving the durable publication. An authorization denial can
therefore precede an idempotency conflict on a mismatched retry. These service
methods do not change the REST API or storage contract.
