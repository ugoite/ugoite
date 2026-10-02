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
set. E2E-specific unknown-version and broken-reference documents, Space seed,
and expected-output files wait for Lane A's restricted format, typed schema,
canonicalization, diagnostics, and resource limits to be reviewed and frozen.
The D0 fixture directory records their paths and recovery purpose without
choosing persisted fields or parser behavior ahead of that contract.

## CLI acceptance shape

The planned command surface is:

- `ugoite composition list`
- `ugoite composition inspect <id> [--revision] [--raw]`
- `ugoite composition lint <file>`
- `ugoite composition save <file>`
- `ugoite composition query <id> --param k=v`
- `ugoite composition export <id> --output <path>`
- `ugoite composition import <file>`

Acceptance checks will prove authorized paging for list, exact-revision inspect,
stable diagnostic codes from the shared Rust contract, and save success only
after the Entry receipt confirms the new revision. Raw inspect and export must
remain available for unknown versions and broken references. Query must invoke
the existing resolved query path and preserve paged results. Core and remote
CLI modes must agree on output meaning. JSON field names and presentation
snapshots wait for the shared DTO to freeze.

## Browser Golden Journey shape

With model connection disabled, the Browser saves a Composition in the active
Space, closes the browser context, reopens Home, and opens that saved tool.
Changing a declared parameter runs the source through the existing paged query
path; advancing a page displays rows returned by that query without client-side
aggregation. A delayed old response cannot replace the currently selected
parameter result. Page, scroll, result, continuation, and cache state stay in
disposable Work and do not appear in the saved Entry.

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

## Run evidence

For every executed acceptance selector, record the exact source SHA, command,
selector, surface, fixture, environment, result, artifact, and evidence gap.
Keep each surface result separate. A static locator, mocked operation, or D0
plan check is not a Browser or real-server journey pass. The acceptance map
contains no runtime evidence record yet.
