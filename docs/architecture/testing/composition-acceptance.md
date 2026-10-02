---
title: "Composition acceptance plan"
---

This is the Lane D acceptance plan for the planned Composition feature. It does
not claim a shipped API, CLI command, Browser route, or passing runtime journey.
The current structured locator map is
[`e2e/fixtures/composition/acceptance-plan.json`](../../../e2e/fixtures/composition/acceptance-plan.json).

## Contract and evidence boundary

The Composition Requirement and Feature remain `planned` until implementation
and runtime evidence land. The reserved selectors in the locator map are not
Mitase verification bindings until the corresponding tests exist. The current
D0 check validates only the completeness and pending status of that map.

The canonical monthly-expense YAML document is shared with the domain fixture
set. Two small raw-payload candidates now cover an unsupported format version
and a missing Form reference. They use the merged typed document shape but are
not parser or runtime evidence; the restricted parser rules and resource
limits remain in progress. Space-seed and expected-output files stay planned
until their integration contracts are reviewed and frozen.

## CLI acceptance shape

The planned command surface is:

- `ugoite composition list`
- `ugoite composition inspect <id> [--revision] [--raw]`
- `ugoite composition history <id>`
- `ugoite composition restore <id> --revision <revision>`
- `ugoite composition lint <file>`
- `ugoite composition save <file>`
- `ugoite composition query <id> --param k=v`
- `ugoite composition export <id> --output <path>`
- `ugoite composition import <file>`

Acceptance checks will prove authorized paging for list, exact-revision inspect
and history, append-only restore with its new revision receipt, stable
diagnostic codes from the shared Rust contract, and save success only after the
Entry receipt confirms the new revision. Raw inspect, export, and history must
remain available for unknown versions, malformed documents, and broken
references. Query must invoke the existing resolved query path and preserve
paged results. Core and remote CLI modes must agree on output meaning. JSON
field names and presentation snapshots wait for the shared DTO to freeze.

## Browser Golden Journey shape

With model connection disabled, the Browser saves a Composition in the active
Space, closes the browser context, reopens Home, and opens that saved tool.
Changing a declared parameter runs the source through the existing paged query
path; advancing a page displays rows returned by that query without client-side
aggregation. A delayed old response cannot replace the currently selected
parameter result or change success, error, finalization, or loading state after
a parameter, Space, or source switch. Page, scroll, result, continuation, and
cache state stay in disposable Work and do not appear in the saved Entry.

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
operation names are listed in the fixture acceptance map; request and response
DTOs are intentionally not reserved here.

## Recovery and authorization acceptance

The structured plan adds pending cases for concealed direct Composition reads,
the shared `source_unavailable` projection, current ACL checks after resolve
and on every continuation page, exact raw recovery, append-only history and
restore, generic-write protection, save-receipt reconciliation, and stale
Browser request state. Selectors that depend on storage or resolver service
APIs remain unbound until those public contracts settle. No DTO field names are
reserved by this plan.

`composition.save` stays disabled until generic Entry create, update, bulk,
import, and restore paths cannot bypass Composition validation, and the
receipt/reconciliation path can establish the exact published revision. A lost
save response remains outcome-unknown until that reconciliation succeeds; a
retry must not publish a duplicate. Generic restore of the reserved Form is
denied while Composition-scoped restore validates and appends a new revision.

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
journey pass. The acceptance map contains no runtime evidence record yet.
