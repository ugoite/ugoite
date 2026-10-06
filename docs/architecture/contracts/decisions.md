---
title: "Architecture decisions"
---

These accepted decisions define the implementation boundaries that contributors
and adapters must preserve.

## ADR-001 — Rust is the canonical implementation

**Accepted.** Domain, storage, application behavior, REST, CLI, and WASM
protocol logic live in one Rust workspace. Adapters must not reimplement use
cases in another runtime.

## ADR-002 — Space files are authoritative

**Accepted.** A Space directory/prefix is the portable source of truth. Indexes,
projections, and SQL query continuations are disposable. Backups operate on the
complete Space prefix; pre-release format migrations are unsupported. Node
control state and the node secret are separate node-local recovery inputs.

## ADR-003 — Forms provide typed Markdown structure

**Accepted.** Entries remain Markdown-oriented while Forms define fields and
validation. Entry and revision records are stored through Form-specific
structured tables.

## ADR-004 — Remote operation semantics are portable

**Accepted.** `ugoite-api-client` owns operation names, method/path/body/auth
intent, and decoding. It performs no I/O. Native and browser runtimes supply
transport.

## ADR-005 — Current browser is server-backed

**Accepted current state.** JavaScript/WASM calls the Rust server. Browser-local
persistence and sync are a future runtime adapter, not an existing
implementation.

## ADR-006 — One release image

**Accepted.** The runtime image contains server, CLI, and static browser files,
runs as non-root, and mounts `/data`.

## ADR-007 — MCP is a small stable semantic facade

**Accepted.** MCP v1 exposes only `ugoite.search`, `ugoite.save`, and
`ugoite.delete`, filtered by authorization, plus lazy semantic Entry, history,
schema, and Form resources. It does not mirror REST endpoints or expose
storage/revision details; broader tools remain future work.

## ADR-008 — Iceberg-native workspace model

**Accepted.** A Space maps to one Iceberg namespace. A stable Form ID maps to
one physical `form_<uuid>` table; display-name changes never rename the table.
The Iceberg schema owns field IDs, types, and nullability. Versioned Ugoite
labels, validation, references, and semantic metadata live in table properties.

## ADR-009 — Entry history is the authority

**Accepted.** Create, update, delete, and restore append revision rows to the
Form table. Current state is the unique greatest `entry_version`; equal maximum
versions are a conflict. There is no authoritative current-entry table and no
two-table commit.

## ADR-010 — OpenDAL-published SpaceCatalog and DataFusion

**Accepted target.** A Space has one OpenDAL-backed `SpaceCatalog` implementing
the current Iceberg `Catalog` trait. Its sole mutable authority is
`_ugoite/catalog/head.json`; immutable linked publication records beneath
`_ugoite/catalog/publications/` provide the evidence needed to resolve an
ambiguous Head compare-and-swap after later writers publish.

Shared writes require a real Head ETag, exact `if_match` reads, conditional
initial creation, conditional whole-object replacement, stale rejection, and
one-winner concurrent CAS proven by backend probes. ETags are opaque tokens.
Non-local stores remain `SharedReadOnly` until the probe promotes them to
`SharedVerified`; unsupported or unverified stores fail closed. An explicitly
selected single-process mode may serialize in-process while retaining every
durable byte through OpenDAL. Readers never lock, and no lease, heartbeat, TTL,
lock file, fencing token, external Catalog, or relational database is part of
the production architecture.

Iceberg requirement/update application, filenames, and I/O use the current
official Iceberg Rust stack. Ugoite's logical-coordinate FileIO bridge accepts
only `ugoite://{space_uid}/{space-relative-key}`, resolves it against the
operator bound to the Space, and rejects malformed or cross-Space locations. It
never persists the physical warehouse URI or infers metadata from listings.
DataFusion is the standard structured query engine and receives
authorization-filtered, snapshot-pinned Iceberg providers. One Form-table commit
is atomic; cross-Form transactions are explicitly unsupported.

## ADR-011 — Portable logic is not a storage adapter

**Accepted.** `ugoite-domain` owns stable IDs, Form changes, compatibility,
revision construction, and I/O-free validation. WASM exposes that logic and the
portable API protocol; it never depends on Iceberg, Arrow, Parquet, OpenDAL, or
a native repository interface.

## ADR-012 — DerivedRelation is a sibling of Form

**Accepted.** A DerivedRelation is a typed Iceberg relation whose current
materialization can be deleted and rebuilt from authoritative Space data. Each
relation has an independent `_ugoite/derived/relations/{relation_id}/head.json`
published with OpenDAL conditional semantics. DerivedRelation updates never
change the main Catalog Head, Form revision history, Form registry generation,
or SpaceCheckpoint coordinates. AssetText is internal to trusted Quick Search
and is not exposed to Saved SQL, SQL queries, or authorization.

The first producer uses replace-all full rebuilds. Persistent substring inverted
indexes, cross-relation transactions, OCR, and a mandatory external job queue
are separate future work.

## ADR-013 - Konase is a client-side effect-driven control plane

**Accepted current slice.** ugoite-konase owns Work, Job, Observation, Context
Capsule, and deterministic Event/Effect transitions. It performs no network,
filesystem, storage, async runtime, or provider I/O. Native and WASM hosts
execute serializable effects and return results as events.

Konase state, agent memory, and raw model context are disposable client state.
Meaningful Knowledge outcomes continue through the existing Ugoite Space and
Change/Run/Undo semantics. A future View or Application Definition may be
Space-owned Knowledge, but its renderer and runtime state remain replaceable
Experience. Provider/framework implementations, Agent Plugins, MCP transport,
CLI UI, and browser UI remain replaceable adapters and are not part of this
slice.

## ADR-014 — SQL Form names resolve at execution and Saved SQL binds by Form ID

**Accepted.** A normal stateless SQL request resolves a quoted Form name against
the Forms already authorized for that request's pinned Publication. A SQL
relation named `"Expense"` therefore identifies the uniquely matching Form in
that Publication at execution time. Name matching is exact and case-sensitive.
Form names use the existing identifier grammar (ASCII letters, digits, `_`, and
`-`; digits may be first), and name references must use SQL double quotes; an
unquoted identifier is not a Form-name reference. A missing or ambiguous name is
an error. SQL parsing and relation collection use the Rust SQL parser, never
SQL-text replacement.

The legacy `form_<UUID>` relation remains valid and continues to identify the
same physical Form relation. Physical table names and `sql_relation_name()` do
not change. A Form name that collides with a reserved ID-shaped relation is
ambiguous and fails closed; neither the name nor the internal relation silently
wins. Name lookup only chooses among authorized Forms and never grants access.
Existing logical/physical plan authorization and Publication isolation checks
remain mandatory after resolution.

Saved SQL stores its original SQL text and a versioned set of resolved Form
bindings in that saved revision's integrity-protected metadata. The service
derives the bindings when creating or updating Saved SQL; client-supplied
bindings are not trusted. At execution, the saved SQL ID and revision select the
exact historical payload and bindings. Each bound Form ID must still exist in
the selected Publication and remain authorized. A missing, renamed, or
unauthorized bound Form fails closed; it never falls back to a same-name Form.
The SQL text is not rewritten on Form rename. Diagnostics expose the bound name
and current Form name so a user can explicitly edit and save a new revision.

The existing Saved SQL metadata object is the version boundary. Readers that do
not recognize a binding version or its metadata fields fail closed because the
Saved SQL metadata decoder rejects unknown fields. They must not return an
apparently usable definition or append a revision that omits the bindings. Older
Saved SQL revisions without binding metadata remain readable and retain their
current behavior, including `form_<UUID>` references. No migration or rewrite
occurs during read. This forward-reader boundary is intentional for the pre-1.0
format; mixed-version write access to a Space containing bound Saved SQL is
unsupported. Rollback means deploying a reader that understands the binding
format, not reopening the Space with an older binary.

Query page, count, export, and continuation execution share the Rust SQL
execution path. A continuation remains pinned to its Publication and SQL
fingerprint and additionally carries the saved revision identity and resolved
bindings when the request executes Saved SQL. Authorization is reevaluated on
each continuation request.

## ADR-015 — Shared authorization and content publication coordination

**Accepted target; implementation is a release blocker.** S1 is selected for
shared multi-process Space writes. A per-Space coordinator Head updated by one
backend-enforced exact CAS is the linearization point for both authorization
snapshot changes and protected content publication. Prepared content remains
unreachable until that CAS. Permission evaluation uses the authorization
snapshot referenced by the exact Head being advanced. A single-use approval
consumed by a content operation advances with that content root in the same CAS.
Per-object CAS between separate authorization and content objects is
insufficient.

Every `Authorizer` state writer must publish through the coordinator boundary.
The maintained inventory covers owner initialization; approval issue,
consumption, audit queue, and delivery acknowledgement; recovery-fence reserve,
complete, and release; policy, membership, role, and principal changes; and
agent create, recovery, revoke, and use. A boundary test must detect a writer
that bypasses the shared authorization-state publication helper.

The exact Head revision is the fencing token; owner lease expiry, heartbeat
loss, timeout, or cleanup cannot authorize a stale Head replacement. An unknown
publication outcome is reconciled from the exact Head and canonical receipt; if
it remains unknown, the write stops without automatic resend. This prioritizes
safety over availability when a writer cannot establish its outcome. The
supported-backend matrix and S1 acceptance contract are maintained in
[`shared-authorization-publication.md`](../security/shared-authorization-publication.md).

## ADR-016 — Composition is Space-owned Knowledge

**Accepted target contract for v0.2.2.** A saved Composition is one Entry
revision in its user-owned Space. The Space remains its authority; Entry
history, restore, receipts, and stale-write conflicts remain the persistence
contract. A separate database or hidden catalog is not a Composition authority.
The initial executable kind is `dashboard`. The initial
save/list/get/history/resolve persistence operations, the CLI commands, and the
Browser save/list/resolve/query paths have landed with focused bindings and
evidence (see the [Composition contract](composition.md) and the
[acceptance plan](../testing/composition-acceptance.md); the complete journey
acceptance, plus chart DSL, layout engine, actions, Konase generation, and
proposals store, remain future work.

Unsupported format versions and broken source references may prevent execution
but must not remove raw inspection, export, or revision history. Browser page,
scroll, result, and cache state are transient Work and are not stored in a
Composition.

## ADR-017 — Composition semantics use a restricted YAML contract

**Accepted.** `.ugcomp.yaml` v1 is a restricted exchange representation with the
ordered envelope `format`, `format_version`, `kind`, `name`, `tags`, and `spec`.
The fixed format marker is `ugoite.composition`. Rust typed domain values own
its meaning; native and WASM callers use the same parser, validation,
diagnostics, canonicalization, and fingerprint behavior. Adapters do not add a
second Composition parser or query engine. The document's tags are the same
ordered tag list written to the owning Entry.

An exact Saved SQL source carries an ordered `expected_result` descriptor with
unique output names and portable logical types (`string`, `boolean`, `integer`,
`float`, `date`, `timestamp`, or `json`). It describes an expected result and
does not store SQL-backend type names. The resolver derives source-result
fingerprints from exact revision identity, the normalized descriptor, and bound
variable schema. No source-result fingerprint is stored in the document.

The parser uses `serde-saphyr` 1.3.0 with duplicate-key errors, strict typed
deserialization, and explicit resource budgets. It rejects multiple documents,
anchors and aliases, merge keys, unsupported/custom tags, and unknown fields. It
limits input to 65,536 bytes, nesting depth to 64, YAML nodes to 4,096, parser
events to 8,192, total scalar bytes to 32,768, and each Composition collection
to 256 items. A bounded probe checks the fixed format marker and reads
`format_version` before strict v1 deserialization. The numeric limits and
candidate comparison are maintained in [`composition.md`](composition.md).

Canonical output derives from normalized typed values, uses schema field order
and stable YAML formatting, and does not preserve comments. The semantic
fingerprint is SHA-256 over the normalized semantic value, not the source YAML
bytes. Normalization sorts each `entry_query` field-schema snapshot by
`FieldId`, rejects duplicate or inconsistent entries, and includes a typed
List's `items` definition and a RowReference's `reference_form` when present.
The fingerprint hashes compact JSON serialization of the normalized typed value;
the public digest is lowercase hexadecimal. Rust's domain and WASM paths return
the same canonical YAML, fingerprint, and diagnostic code for a given fixture.

The complete current contract is maintained in
[`composition.md`](composition.md); the shared example is
`crates/ugoite-domain/tests/fixtures/composition/monthly-expense.ugcomp.yaml`.

Local CLI operator authority remains distinct from remote Server principal
authorization. The storage layer supplies conditional publication mechanics;
`ugoite-iceberg::authorization` remains the owner of ACL state and decisions.

## ADR-018 — Composition Studio preview and typed SQL result schema

**Accepted ruling for the v0.2.2 Composition Studio uplift.** This ruling
authorizes two additive extensions without reopening the Composition v1
grammar freeze recorded in [`composition.md`](composition.md):

1. `composition.preview`: a side-effect-free authoring adapter that parses a
   candidate document, applies current source authorization, resolves through
   the shared Core resolver, and executes only the existing `entry.query` and
   `sql.query` operations. It creates no registry, history, or publication
   state; it stores no fingerprints; it changes no query grammar. It requires
   current ACL exactly like saved-revision resolution.
2. `sql.query` `result_schema`: a backward-compatible response extension
   carrying the server-owned portable logical column types (`string`,
   `boolean`, `integer`, `float`, `date`, `timestamp`, `json`) mapped from
   DataFusion/Arrow output in Rust. Existing `columns` and `rows` are
   unchanged. The Browser performs no type inference and keeps no type
   mapping; it stores the descriptor unchanged in the Composition
   `expected_result`. Null and empty results retain their types;
   duplicate or ambiguous columns fail closed.

Document grammar, storage layout, mutation operations, query grammar, metric
exact-scalar semantics, generic write guard, idempotency semantics, raw
recovery, downgrade compatibility, and the `string` spec carrier are
unchanged. The operation inventory and freeze tests are updated in the same
change that introduces `composition.preview`.

## ADR-019 — Composition layout v1 ruling (sections to layout AST)

**Accepted ruling for the v0.2.2 Composition Studio redesign.** The semantic
vocabulary `parameters` / `sources` / `components` is shared with the future
v0.3 Dashboard / Application / Presentation / Master grammar and is kept.
The pre-release `sections` array is removed and replaced by a first-class
`layout` AST for `kind: dashboard`:

```yaml
layout:
  kind: flow
  rows:
    - id: controls
      items:
        - kind: parameter
          parameter: month
    - id: summary
      items:
        - kind: component
          component: total
```

Rules recorded here, implemented starting with the domain reset that follows
this ruling:

1. `format_version` stays `1`; no migration reader is built for the
   pre-release `sections` grammar.
2. Components are `text` / `metric` / `table` only. `text` carries a fixed
   `style` enum (`title`, `heading`, `body`, `caption`); no Markdown, HTML,
   CSS, or handlers. Parameters are never duplicated as components; a layout
   item of `kind: parameter` places the semantic parameter as a control.
3. Every component is referenced exactly once from the layout; empty layouts,
   empty rows, duplicate row IDs, unknown references, duplicate placements,
   unreferenced components, and dangling parameter controls are
   `invalid_composition`.
4. Pixel coordinates, CSS classes, DOM paths, and canvas state are never
   Knowledge; row/item order is the only placement contract. Desktop renders
   row items inline, mobile wraps/stacks them.
5. Studio save requires at least one layout item; a source-only document
   cannot be saved from the Browser Studio.
6. Studio IA is `Design` (finished-Tool look, edit affordances only) /
   `Data` (per-source EntryQuery / exact Saved SQL viewers) / `Inspector`
   (selected block only), plus desktop-only `Split`. Studio and Use surfaces
   share one renderer.

This ruling authorizes the `composition.md` v1 re-freeze once the domain,
resolver, DTO, renderer, Studio, seed/save-readiness, mobile, history, and
story-E2E work lands. Until then the frozen v1 pin above still describes the
shipped grammar. Stop conditions: canvas coordinates in Knowledge, split
Design/Use semantics, parameter duplication into a UI-only type, Saved SQL
latest-fallback, Browser aggregation or type inference, `text` runtime
extension, or a permanent `sections` compatibility layer.
