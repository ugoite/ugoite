---
title: Composition contract
description: The portable document and semantic boundaries for Composition v1.
---

**Status:** accepted semantic boundary; parser and resource limits were selected
from the COMP-011 native and WASM spike.

## Portable document

A Composition is a user-owned Knowledge document represented as one Entry
revision in its Space. The Space remains its authority. History, restore,
receipts, and stale-write conflicts use the existing Entry revision contract;
no separate database or hidden catalog becomes authoritative.

The exchange format is a restricted `.ugcomp.yaml` document at format version
`1`. Its envelope contains `format_version`, `name`, `kind`, and `spec`; `kind`
is `dashboard` for v0.2.2. The `spec` contains typed parameters, sources,
components, and sections. A source identifies either an Entry query template
or an exact Saved SQL revision. Components refer to named sources and sections
group named components. An `entry_query` source carries a sorted `field_schema`
snapshot for the union of its query-used property fields and, when text search
is present, every text-searchable property field. The text-search expansion
follows the existing EntryQuery semantics: `binary`, `list`, `object_list`, and
`asset_reference` fields are excluded. A field of those types still appears
when the query otherwise uses it, such as in a projection. Each snapshot row
contains `field_id` and `field_type`, plus `reference_form` for a RowReference
or `items` for a typed List when those type details are present.

The complete illustrative document is maintained in
`crates/ugoite-domain/tests/fixtures/composition/monthly-expense.ugcomp.yaml`.
It is a contract fixture for domain, parser, canonicalization, and WASM parity
tests, not an assertion that persistence or rendering is already shipped.

The typed v1 model supports `string`, `boolean`, `integer`, `float`, `date`, and
`timestamp` parameters. A parameter may carry a typed default and a display
format hint; `year-month` is a hint and does not evaluate an expression or
change the parameter value. Source values are literals or named parameter
references. An `entry_query` template scopes property `FieldId`s to its
`FormId`, snapshots query-used field types, and carries the existing query
filters, sort, text, projection, and page-size semantics. Text may be a literal
or parameter reference; a parameter used for text is bound as a string. Its
projection is either preview or an ordered list of property `FieldId`s. The
optional `page_limit` defaults to 100 and is still bounded by the existing
EntryQuery maximum. A `saved_sql` source carries an `EntryId`, an exact
`RevisionId`, and literal-or-parameter variables. Dashboard components are
`metric` or `table` and refer to named sources; sections group named
components.

The stable diagnostic codes are `unsupported_format_version`,
`invalid_composition`, `parameter_unknown`, `parameter_missing`,
`parameter_type_mismatch`, `source_unavailable`, `missing_field`,
`field_type_changed`, and `source_schema_changed`. A missing or denied
Composition uses the same existing generic 404/error response shape, so its
existence is not disclosed. `not_authorized` is not a Composition diagnostic.
After Composition access is authorized, a missing or denied Form or exact
Saved SQL source has the same caller-visible `source_unavailable` diagnostic,
with no source ID or metadata. Field-level diagnostics are returned only after
the source Form is authorized. UI messages are supplied by the consuming
surface and are not part of this domain contract.

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
domain parser first performs bounded version inspection, then strictly parses
supported v1 input; both passes use `serde-saphyr`.

The parser accepts at most 65,536 input bytes, 64 levels of nesting, 4,096
nodes, 8,192 parser events, 32,768 total scalar bytes, and 256 items in any one
Composition collection. The version probe accepts at most two documents and
limits anchors, aliases, merge keys, and retained anchor data; strict v1 parse
rejects those constructs outright. These limits leave substantial room over
the shared monthly-expense fixture, which measured 1,475 input bytes, 144
nodes, 181 events, depth 8, and 842 scalar bytes. The ignored profile test
reported 1,000 full parser calls in about 1.07 seconds in a debug build on the
implementation host. This is a reproducibility sample, not a performance
threshold.

Canonical YAML uses schema field order, block collections, two-space
indentation, LF line endings, and no BOM. Canonical output is derived from the
normalized typed value; comments are not preserved. The semantic fingerprint is
lowercase hexadecimal SHA-256 over the normalized typed value serialized as
compact JSON. Formatting changes do not change identity. The `field_schema`
sequence is unique and normalized by `FieldId`; its fingerprint contains only
the source Form ID and used field IDs with their logical type, List item, and
RowReference target metadata. Text search alone does not make List fields used
because existing EntryQuery semantics exclude them from text-search expansion.
The shared native/WASM fixture can be executed with
`deno run -A crates/ugoite-wasm/tests/composition_parity.ts`.

## Failure and recovery boundary

Unsupported format versions and invalid documents return stable diagnostic
codes. They do not make raw inspection, export, or revision history unavailable.
The parser identifies `format_version` before strict v1 typed deserialization;
only a supported v1 document is deserialized as `CompositionDocument`. Raw
inspection, export, and history use the stored revision independently, so an
unsupported or malformed document remains recoverable. Broken source
references are execution failures; they do not erase the stored document.

Entry query templates resolve to the existing bounded EntryQuery contract.
Saved SQL references identify an exact Entry and Revision; they never fall
back to the latest revision. Current authorization is re-evaluated by the
existing query paths.

Browser page, scroll, result, and cache state are transient Work. They are not
fields of a Composition and do not become Space Knowledge.
