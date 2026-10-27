---
title: "ADR-023 — Typed reactive Composition variables"
---

**Status: Historical prerelease target; superseded by ADR-024 before release.**
This decision recorded a breaking change to the unreleased Composition v1
grammar; its implementation was replaced before release. ADR-024 records the
current prerelease implementation, and ADR-025 defines the accepted next
target. This ADR does not define a compatibility contract or authorize a
product release. Its SQL and Metric boundary inherited ADR-022.

This ruling supersedes ADR-019 only where it requires parameters and parameter
layout items, restricts Components to Text/Metric/Table, and forbids variable-
backed Text. ADR-019 still governs the flow Layout AST, exactly-once Component
placement, presentation order, text style enum, and Design/Data/Inspector
separation. ADR-022 remains the sole Metric execution ruling.

## Context

Composition currently declares parameters separately from placed controls,
Text is literal-only, and Browser query state is parameter-specific. That model
cannot express reusable typed values whose controls, Text, and data sources
stay synchronized without user-authored handlers. The intended app needs an
explicit dependency model: Inputs write one Composition-scoped value, Text
reads it synchronously, and only dependent Saved SQL sources re-evaluate.

Composition remains user-owned Knowledge represented by one Entry revision.
Values entered while using or previewing a Composition and query results are
transient Work.

## Decision

### 1. Original pre-release document grammar decision

The original typed-variable ruling kept `format_version: 1`, declared unique
stable variable IDs, required types and optional typed defaults, and put Input,
Text, Table, Metric, and flow layout in one portable document. ADR-024 replaces
its source and presentation details with the SQL-first v1 schema: variables do
not have labels, and the only sources are inline SQL and exact Saved SQL
revisions. ADR-024 also defines the replacement's strict reader and recovery
boundary. This section records why typed reactive variables are part of the
Composition model; its earlier EntryQuery and label wording is not a current
schema requirement.

This decision is limited to prerelease Composition. Existing Space, Entry,
Form, Saved SQL, SQL API, authorization, history, and publication contracts
remain unchanged.

### 2. Variables own values; Input components are controls

The initial types are string, boolean, integer, float, date, and timestamp.
Type is mandatory. Defaults must have that exact type. Do not infer types,
coerce values, or add nullable variables in this phase. Unset is not empty
string, zero, or SQL NULL. A required variable without a default needs a
placed Input in an interactive Composition. An explicit unset overrides its
default; resolving a required unset returns `variable_missing` with the
variable ID. The Browser keeps dependent Sources invalid, shows no prior page
as current, and does not query those Sources until the variable is valid.

An Input contains a component ID and a reference to one declared variable. It
stores no second value, display label, arbitrary input type, setter, or event
expression. Multiple Inputs may reference the same variable. Variable IDs are
stable references, not primary UI labels.

#### 2.1 Typed Input values are bounded and locale-independent

For the current ADR-025 variable graph, ADR-026 supersedes only the integer
token length and value bounds in this historical decision and its A25 row.
The remaining constraints and historical evidence remain unchanged.

Use these lexical and value constraints consistently in Domain, Core, Native,
WASM, CLI, and Browser. A variable's optional display-format hint never
changes its accepted type or lexical grammar. A value outside its constraint
stays visible as raw invalid Input state, is never truncated or coerced, and
cannot trigger a SQL request. Defaults are validated by the same value rules
before a Composition can be saved.

- `string`: at most 4,096 UTF-8 bytes. Reject Unicode Cc control characters
  except horizontal tab, line feed, and carriage return. Preserve accepted text
  exactly; do not trim, normalize, or truncate it.
- `boolean`: the typed value is JSON `true` or `false`; textual input accepts
  only the exact lowercase tokens `true` and `false`.
- `integer`: accept the ASCII token `[+-]?[0-9]+`, at most 17 bytes including
  its optional sign, with no surrounding whitespace. Leading zeroes are
  accepted. Parse decimal digits with checked integer arithmetic before any
  JavaScript Number conversion, then require the value to be in the safe
  integer interval `[-9007199254740991, 9007199254740991]`. Reject decimal
  points, exponents, and out-of-range values.
- `float`: accept the ASCII grammar
  `[+-]?(?:[0-9]+(?:\.[0-9]+)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?`, at most 64
  bytes including sign and exponent, with no surrounding whitespace. Require a
  finite IEEE-754 binary64 result. Reject overflow and a nonzero token that
  underflows to zero. This accepts `1`, `1.25`, `.5`, and `1e3`; it rejects
  `1.`, `1,25`, `NaN`, `Infinity`, overflow, and underflow to zero. Finite
  binary64 rounding is the declared `float` type's numeric behavior.
  A finite JSON number with integral spelling, such as `1`, is still a Float
  value. Saved SQL binding checks the declared Composition and SQL variable
  types exactly; it never infers the type from the JSON number's spelling.
- `date`: accept exactly ten ASCII characters in `YYYY-MM-DD` form, with a
  four-digit year from 0001 through 9999, and require a real Gregorian date.
  Do not accept whitespace, reduced precision, or a date-time value.
- `timestamp`: accept the RFC 3339 profile
  `YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)`, with uppercase `T`/`Z`, one to
  nine fractional digits when present, a valid calendar date and time with
  seconds from `00` through `59` (leap seconds are not accepted), and an
  explicit numeric offset or `Z`. Reject offset-free local times and `-00:00`,
  which does not identify a known offset. Do not parse through the browser's
  locale or local time zone.

The accepted timestamp string is passed as a typed SQL parameter without
rewriting SQL text. Browser controls must preserve the explicit offset; a
`datetime-local` value by itself is not a valid timestamp variable. Empty raw
Input means unset, not an empty-string value, zero, or SQL NULL.

Composition source values use one reference form: {variable: id}. A Saved SQL
source maps each SQL parameter name to a Composition variable. Core checks
exact type agreement with the pinned Saved SQL revision before mapping the
typed value to the existing SQL parameters and parameter_types request fields.
SQL text is never rewritten, interpolated, or concatenated.

Runtime requests distinguish omitted values from explicit unsets. Omission
selects a declared default; an Input that parses as unset is named in the
request's `unset_variables` list and overrides that default. An explicit unset
is never encoded as a typed value or SQL NULL.

### 3. Text is literal or a direct string-variable reference

Text content is a discriminated choice: a literal string or a direct reference
to a string variable. Variable-backed Text displays that string without an
expression language, implicit formatting, SQL-result interpolation, or
computation. Changing a variable updates dependent Text in the same reactive
UI update cycle.

### 4. Dependencies, not layout order, control execution

The dependency graph is:

    Input A -> Variable minimum -> typed binding -> exact Saved SQL source
                                      |-> Metric row count
                                      |-> Metric total
                                      |-> Table rows
    Input B -> Variable heading -> Text heading

Resolve variable references into a dependency index. Each declared variable
has one entry in the transient Variables Store containing its raw input buffer,
validation state, and (when valid) typed value. Every Input bound to that
variable renders the same raw buffer and validation state. Editing any bound
Input synchronously updates that one entry and all other bound Inputs and Text;
there are no per-Input copies of a variable's value. A valid value change also
immediately marks only dependent SQL-backed components pending and invalidates
their previous results. Re-execute only Sources whose referenced variable
subset changed. A variable used only by Text must not issue SQL. Reordering
components or rows must not change results.

On every input event, preserve the raw buffer and validation status in the
single variable entry. An incomplete or invalid value, such as a lone minus
sign in an integer input, does not become a guessed typed value and does not
issue SQL. All Inputs bound to that variable continue to display the same raw
buffer and validation status. Required unset or invalid values make dependent
components unavailable without presenting an old success as current. SQL
issuing may use a short debounce (about 100–150 ms) to coalesce calls; input,
Text, validation, and stale/pending invalidation are never debounced. Tests for
out-of-order handling must dispatch at least two requests for one Source while
an earlier request is still unresolved (for example, advance a fake debounce
timer between edits), then resolve them in reverse order. This keeps the
generation check observable even when normal rapid edits are coalesced.

Abort older requests when possible. Independently increment a per-Source
generation and accept a response only if its generation, Composition revision,
variable-subset fingerprint, exact Saved SQL revision, Space/principal
authorization context, and publication context still match. Discard late
responses even when cancellation fails. Failed or denied requests do not fall
back to an older successful value.

### 5. Existing authorized SQL remains the only data-processing engine

Saved SQL retains its exact Entry and revision, typed SQL variables, read-only
execution, current ACL re-evaluation, Form scope, publication checkpoint,
resource limits, and typed results. Do not add client aggregation, SQL string
substitution, a server-side mutable Composition session, or a Metric-specific
calculation engine. SQL parameters and parameter_types names and meanings do
not change.

A Metric remains a display component over a shared SQL source and one selected
scalar output column. It requires exactly one result row and distinguishes
zero rows, one-row NULL, numeric zero, multiple rows, type mismatch, and access
denial. It never selects the first row or adds LIMIT 1. Table continues to use
its existing paged path independently from Metric scalar probing.

### 6. Persistent Knowledge and transient Work stay separate

Persist variable declarations, types, required flags, defaults, Sources,
Components, and Layout in the Composition Entry revision. Do not persist
runtime values, raw input text, previews, SQL responses, loading state, or
errors through implicit save, URL, local storage, or server session. Studio
preview and use start from declared defaults plus the current request snapshot.
Editing runtime input never creates an Entry revision or overwrites a default.

### 7. Cross-surface contract

- Rust Domain/Core are semantic authorities for parsing, type validation,
  references, canonicalization, and diagnostics. Native Rust and WASM use the
  same contract.
- Composition resolve, preview, and Metric DTOs use variables and
  variable_definitions. Stateless SQL DTOs retain parameters and
  parameter_types.
- CLI Composition queries use one-shot --var name=value arguments and do not
  add an interactive reactive session or retain the unreleased --param alias.
- Browser owns transient scheduling and one typed Variables Store. It does not
  infer domain or SQL types, parse another Composition grammar, or implement
  a second SQL engine.
- Studio exposes declarations, Input placement, literal/variable Text choice,
  and exact-type SQL variable binding without requiring code. Composition
  source, variable, and component labels are not persisted; semantic names
  remain available from the SQL object, SQL output, variable ID, and Text
  content. Internal IDs stay out of primary UI paths. Controls remain keyboard
  and screen-reader operable at 390px.

## Acceptance matrix

Each ID is a required named acceptance case. Exact implementation bindings and
test selectors are added in S1–S6; this ADR records the invariant before code
changes.

| ID | Planned test selector | Operation | Required result |
| --- | --- | --- | --- |
| A01 | heading_input_updates_text_synchronously_without_query | Change a string heading through Input | Text updates in the same UI cycle; SQL call count is unchanged. |
| A02 | typed_source_dependency_invalidates_before_requery | Change an integer used by SQL | Text is immediate; dependent Metric/Table becomes pending immediately and only the newest result is shown. |
| A03 | shared_variable_inputs_stay_synchronized | Edit one of two Inputs bound to one variable, first validly and then with an incomplete value | Both Inputs synchronously show the same raw buffer and validation state; the single variable entry is authoritative and holds a typed value only while valid. |
| A04 | layout_order_does_not_affect_dependency_results | Reorder Input and Metric in Layout | Values and query results stay identical. |
| A05 | sql_aggregates_preserve_null_and_empty_semantics | Aggregate [10, 20, NULL] | COUNT(*)=3, COUNT(Amount)=2, SUM(Amount)=30, AVG(Amount)=15. Test aggregate-without-WHERE separately from variable-filter behavior. |
| A06 | shared_saved_sql_columns_bind_to_separate_metrics | One-row, two-column Saved SQL result | Two Metrics select separate columns from the same Source. |
| A07 | multiple_sql_rows_reject_metric_but_page_in_table | Saved SQL returns two rows | Metric rejects scalar cardinality; Table still pages rows; no first-row fallback. |
| A08 | scalar_result_distinguishes_zero_rows_null_and_zero | Compare zero rows, one-row NULL, numeric 0 | Three distinct states/results are preserved. |
| A09 | unrelated_variable_does_not_requery_source | Change variable unused by a SQL Source | That Source is not requested again; Table paging is unchanged. |
| A10 | rapid_input_keeps_only_latest_source_generation | Dispatch two or more requests for one Source while an earlier request remains unresolved (advance fake debounce timers if needed); resolve responses in reverse order | Only the response for the latest Source generation is accepted; earlier responses are discarded. |
| A11 | invalid_raw_input_blocks_query_and_stale_result | With two Inputs bound to one required integer variable that has a default, first establish a successful result, then enter incomplete/invalid input such as -, empty, or 1.5 in either Input | Both Inputs show the same raw buffer; empty overrides the default; no SQL runs; dependent output is unavailable; and prior success is not current. |
| A12 | sql_variable_binding_rejects_implicit_type_conversion | Bind string to SQL date or float to SQL integer | Exact type mismatch; no implicit conversion. |
| A13 | invalid_variable_grammar_fails_lint_and_save | Duplicate/unknown IDs, invalid defaults, unknown references, unbound required variable | Lint/save fails deterministically. |
| A14 | sql_source_uses_typed_parameter_binding | Bind $minimum as a typed SQL parameter | SQL text is unchanged and the value cannot become SQL syntax. |
| A15 | composition_persists_defaults_not_runtime_values | Save, close, revisit | Declarations/defaults persist; runtime changes do not. |
| A16 | composition_revision_change_discards_old_response | Revisit another Composition revision during a request | Exact revision/history holds; old response is discarded. |
| A17 | revoked_source_read_denies_without_stale_data | Revoke Saved SQL or Form Read access | Output is denied without stale value or unauthorized identity/text leakage. |
| A18 | sql_resource_limits_fail_without_partial_success | Exercise overflow, Decimal, high-cardinality, memory limits | Existing SQL errors remain; no partial success. |
| A19 | table_paging_does_not_change_metric_or_variables | Page a SQL-backed Table | Metric and variable state do not change. |
| A20 | composition_cli_vars_match_local_and_remote | Local and remote CLI with --var | Same typed snapshot/result; unreleased --param is rejected. |
| A21 | native_and_wasm_variable_canonicalization_match | Native and WASM canonicalize the same YAML | Canonical bytes/fingerprint match; old Composition grammar is rejected. |
| A22 | composition_inputs_are_accessible_at_mobile_width | Use at 390px with keyboard and screen reader | Inputs and invalid/loading/result states are operable; IDs are not primary labels. |
| A23 | studio_preview_drops_stale_draft_and_variable_generation | Change draft and variables while Studio preview is in flight | Only matching draft fingerprint and variable generation can update preview. |
| A24 | composition_runtime_isolation_includes_space_and_principal | Use same IDs/values in different Spaces or principals | Responses, cache identity, and authorization never cross-contaminate. |
| A25 | typed_variable_lexical_limits_and_temporal_values_are_strict | Exercise each typed Input at its accepted boundary and one value outside it | String byte/control limits, exact boolean tokens, integer acceptance of `+0000000000000001` at 17 bytes and rejection of `+00000000000000001` at 18 bytes, surrounding whitespace, decimal/exponent forms, and values outside the safe-integer range, float tokens at 64/65 bytes, finite overflow, underflow to zero, real calendar dates, and explicit-offset RFC 3339 timestamps are enforced consistently; raw invalid text is preserved without SQL execution. |
| A26 | variable_filter_changes_golden_aggregate_values | Run the expense journey with minimum 0, then 20 | The [10, 20, 30] fixture yields count 3 / sum 60, then count 2 / sum 50; the separate no-filter NULL aggregate case remains unchanged. |
| A27 | required_unset_keeps_resolution_diagnostic_and_blocks_source | Load a Composition with a ready dependent Source, then explicitly unset its required variable | Resolution returns `variable_missing` for that variable; the prior Source page is hidden and no new Source request runs. |

## Delivery order

1. S0 — This ruling, planned Requirement, semantic Feature, and public issue.
2. SQL-only prerequisites — Settle ADR-022 implementation and shared authorized
   SQL preparation/resource and aggregate tests before dependent Metric work.
3. S1 — Domain: canonical variable/Input/Text grammar, rejection of the
   unreleased parameter grammar, fixtures, canonicalization, Native/WASM parity.
4. S2 — Core: typed snapshots, exact SQL bindings, dependency semantics
   independent of Layout.
5. S3 — Service/adapters: Composition DTOs, API client, OpenAPI, CLI --var,
   and WASM parity around the same snapshot.
6. S4 — Browser: one transient Variables Store, synchronous Input/Text
   updates, dependent Source invalidation, cancellation, generations, and stale
   response protection.
7. S5 — Studio: declaration, Input placement, Text references, and typed SQL
   binding through the existing Saved SQL flow; preserve defaults on save.
8. S6 — Integration: exact Mitase verification per facet, Native/WASM parity,
   CLI local/remote checks, focused tests, and a real Browser/server/Space
   golden journey. Run required CI and report unmet cases. Do not release.

No intermediate main revision may claim both grammars as supported. Preserve
the shared Source/result boundary: Variable -> typed authorized SQL ->
Table/Metric, and Variable -> direct Text.
