---
title: "Composition acceptance plan"
---

This is the Lane D acceptance plan for the Composition feature. One Browser
Golden Journey has passed against the exact source commit recorded below; the
full surface and release evidence remain incomplete. The portable list, get,
history, lint, resolve, and save operations are implemented, as are the CLI
list, history, inspect, lint, and query commands. This does not claim a complete
Composition API or CLI command set or that all Browser acceptance selectors
pass.
The Rust domain parser and Core resolver have focused implementation and test
evidence, documented below and bound in the Mitase feature. The current
structured locator map is
[`e2e/fixtures/composition/acceptance-plan.json`](../../../e2e/fixtures/composition/acceptance-plan.json).

## Contract and evidence boundary

The Composition Requirement and Feature remain `planned` until implementation
and full-surface runtime evidence land. Focused domain and Core resolver tests
are bound in `docs/mitase/features/composition.yaml`; they establish those Rust
contracts only. The reserved selectors in the locator map are not Mitase
verification bindings until the corresponding integration tests run. The D0
check validates the map, fixture presence, selector inventory, and recorded
JUnit artifact integrity. Playwright `--list` separately confirms test
discovery.

The canonical monthly-expense YAML document is shared with the domain fixture
set. Two small raw-payload candidates now cover an unsupported format version
and a missing Form reference. They use the merged typed document shape but are
not resolver or persistence evidence. Domain tests cover the restricted parser,
measured resource limits, canonicalization, and fingerprint semantics.
Native/WASM parity is verified by the Domain and WASM test suites; in
particular, `composition_canonicalization_matches_the_native_domain_contract`
compares WASM documents, canonical YAML, fingerprints, and diagnostic codes
with the native Domain contract. See the
[Composition contract](../contracts/composition.md) and its referenced parser
fixtures. The E2E seed manifest at
`e2e/fixtures/composition/space-seed/manifest.json` now supplies the server
seed, expected result counts, save-retry outcome, and Browser selectors. The
Playwright journey consumes it as test input. Its existence and selector
binding do not record a runtime pass; the focused Browser run remains pending
integration of the full journey and is recorded only after it executes
successfully.

## Resolver implementation evidence

The Rust Core resolver binds typed parameters and compiles `entry_query`
sources into the existing bounded EntryQuery request. It compiles `saved_sql`
sources against the exact current Entry and Revision descriptor, binds the
declared variables, and fingerprints the ordered result descriptor, used
variable schema, exact revision, and selected metric column. A missing or
mismatched current Saved SQL descriptor does not trigger a latest-revision
fallback. These behaviors have focused Core tests recorded in the Mitase
feature bindings.

Metric component bindings use the source's typed field or result-column
descriptor. The Core page adapters validate one already-authorized page as one
complete row with exactly one selected scalar value, using the shared Domain
evaluator. They do not execute a query, fetch another page, or aggregate rows.
This is source-level resolver and page-validation evidence; it does not prove
that CLI or Browser query orchestration, result rendering, or the Golden Journey
is complete.

## CLI acceptance shape

The current CLI command surface includes:

- `ugoite composition list`
- `ugoite composition history <id>`
- `ugoite composition inspect <id> [--revision] [--raw]`
- `ugoite composition lint <file>`
- `ugoite composition query <id> [--revision] [--param k=v]`

`composition list` reads one bounded page in local Core or remote mode. Its
default page size is 100 with offset 0; `--limit` and `--offset` select a
different page. It returns the portable summary DTO and does not load the
stored YAML spec. CLI list and structured inspect output are JSON; inspect
`--raw` writes the stored spec value. Offline lint returns its canonicalization
or diagnostic result as JSON.

`composition history` reads the raw append-only revision page in local Core or
remote mode. Its default page size is 100 with offset 0; `--limit` and
`--offset` select a different page. It returns the shared
`CompositionHistoryPage` JSON, including each raw revision carrier without
parsing its stored spec.

The remaining planned command surface is:

- `ugoite composition restore <id> --revision <revision>`
- `ugoite composition save <file>`
- `ugoite composition export <id> --output <path>`
- `ugoite composition import <file>`

Focused CLI tests cover bounded local list and history pages, remote operation
request and response decoding, summary output shape, exact-revision inspect,
and shared lint diagnostics. Full cross-surface list/get/history acceptance
and CLI parity acceptance remain planned. History and restore must preserve
append-only revisions, and save success must wait for the Entry receipt. Raw
inspect, export, and history must remain available for unknown versions,
malformed documents, and broken references. Query must invoke the existing
resolved query path and preserve paged results. Core and remote modes must
agree on output meaning across the complete command set.

## Browser Golden Journey shape

With model connection disabled, the seeded Browser journey creates the Space,
source entries, and a parameterized Saved SQL query through the existing server
APIs. The user saves the successful query as a tool through the Browser dialog.
The E2E delays and interrupts the first save response after the server commits,
then retries with the same `Idempotency-Key`; the replayed receipt and revision
must match the first commit and history must contain one publication. A fresh
Browser context rediscovers that exact revision from Home and applies the
Composition's declared parameter defaults. It changes its parameters and reads
bounded result pages through the existing query path. Page, scroll, result,
continuation, and cache state remain transient Work and are not saved with the
Composition.

When implementing the surface, follow the existing Mitase UI contract:
structure comes before explanation, each datum has one visible owner, and
each action has one operative control. Supplementary text is used only to
resolve ambiguity about the next action. The renderer reuses
`PagedResultTable`; a metric displays the one scalar returned by the query
engine without aggregating rows in the Browser.

## API/server implementation locator

The portable operation inventory is mirrored by
`crates/ugoite-api-client/src/lib.rs` and
`frontend/src/lib/ugoite-client/protocol.ts`; operation preparation and decode
tests must change with both inventories. Server handlers and `/openapi.json`
remain the HTTP implementation and contract. Composition handlers must delegate
authorization and behavior through the existing service boundary. The D1
operation names are listed in the fixture acceptance map. `composition.get`
currently reads the latest raw revision or an exact revision when `revision_id`
is supplied, and `composition.history` reads a bounded raw history page.
Their wire response DTOs are owned by `ugoite-api-client`; the Server converts
the storage reader result at its adapter boundary. `composition.lint` now
validates and canonicalizes a submitted document through the shared Rust domain
parser at `POST /compositions/lint`. It reads and writes no Space state, returns
the normalized document, canonical YAML, and fingerprint on success, and a
stable diagnostic code on failure. Its request body is bounded and the parser
enforces the 64 KiB YAML limit. `composition.list` now returns one bounded,
ACL-authorized summary page through the portable operation and server route;
the CLI uses this operation in remote mode and the matching local Core reader
in local mode. The list DTO omits `spec`. CLI history uses the portable
`composition.history` operation remotely and the local raw history reader in
local mode. Resolve is available as a side-effect-free operation, and
`composition.save` is exposed by the Server API with a publication receipt.
CLI save/export/import and restore remain planned. The Core resolver returns
requests for the existing query path; it does not itself execute them. The
scalar page adapters described above are tested Core functions and are not yet
evidence of a complete CLI or Browser query journey.

## Recovery and authorization acceptance

The structured plan adds pending cases for concealed direct Composition reads,
the shared `source_unavailable` projection, current ACL checks after resolve
and on every continuation page, exact raw recovery, append-only history and
restore, generic-write protection, save-receipt reconciliation, and stale
Browser request state. Selectors that depend on storage or resolver service
APIs remain unbound until those public contracts settle. No DTO field names are
reserved by this plan.

Generic Entry create, update, bulk, import, and restore paths cannot bypass
Composition validation. The Server save operation returns the publication
receipt that identifies the exact revision. Browser and CLI save surfaces
remain planned. If a save response is lost, the outcome remains unknown until
the exact revision is reconciled; retry idempotency is tracked separately.
Generic restore of the reserved Form is denied while Composition-scoped restore
validates and appends a new revision.

Raw inspection, export, and history read the exact stored spec and revision
metadata independently of strict typed parsing. The version probe runs before
v1 deserialization. Unsupported versions, malformed documents, and broken
source references remain recoverable even when they cannot execute.

Direct missing and denied Composition reads preserve the same existing
caller-visible generic error. Once a Composition read is authorized, missing
and denied source references share `source_unavailable` without exposing source
IDs or metadata. Resolve grants no query authorization: the existing query path
rechecks current access during execution and each continuation page.

## Run evidence

For every executed acceptance selector, record the exact source and candidate
SHA, artifact digest, command, selector, surface, fixture, environment, result,
artifact, and evidence gap. Keep each surface result separate. A static
locator, mocked operation, or D0 plan check is not a Browser or real-server
journey pass. The acceptance map records one focused run of
`Browser saves a tool once, reopens its exact revision from Home, and pages
parameterized results with model connection disabled` against source and
candidate commit `482faff2e5303cf6a8abeaefb123abfe79ba7996`. It passed with one
test and no skips. The tracked JUnit result is
[`composition-golden-journey-junit.xml`](../../../e2e/fixtures/composition/evidence/composition-golden-journey-junit.xml),
with SHA-256
`24d90eaac4952fca0351af2783e35f908bbfc6819963f73399ccea8ba3d21431`. The run
verifies Browser save retry after commit, exact-revision reopen from Home, and
parameterized paged results with model connection disabled. It does not verify
stale query response suppression, CLI parity, recovery and ACL selectors,
performance evidence, or exact release-candidate byte promotion. No release
candidate was built or promoted.
