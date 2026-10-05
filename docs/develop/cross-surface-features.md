---
title: "Cross-surface Features"
description: Extend one Knowledge operation across shared Rust semantics and thin adapters.
sidebar:
  order: 5
---

New capabilities should be designed as one Knowledge operation with several
surface variations. Browser, CLI, REST, MCP, and Konase are experiences; none
is a second Knowledge or authorization authority.

## Start with the shared meaning

1. Define the durable Knowledge outcome and its failure/recovery behavior.
2. Put types and validation in `ugoite-domain`.
3. Put transport-neutral operation names, paths, bodies, auth intent, and
   decoding in `ugoite-api-client`.
4. Put application behavior and persistence in `ugoite-core` and its storage
   services.

The Rust layer must preserve Space ownership, append-only history, and derived
state boundaries. A browser-only store, CLI-only semantic shortcut, or app
database is not an acceptable second implementation.

## Add thin adapters

- `ugoite-server` exposes the REST route and MCP facade. Its router and
  `/openapi.json` are the REST authority.
- `ugoite-cli` maps local core and remote endpoint modes to the shared
  operation. Exact flags belong to `ugoite <command> --help`.
- `ugoite-wasm` exposes the portable Rust behavior to the frontend without
  owning persistence or transport.
- `frontend` presents the task outcome and uses the portable protocol. Keep
  Browser and CLI variations together in the corresponding `docs/use` page.

Do not copy REST semantics into a second adapter or make UI labels the product
model. The task outcome comes first; the surface is a variation.

## Facet-oriented capability authoring

New and reworked capability features use the facet-oriented pattern over the
unchanged Mitase authoring contract (Mitase 0.2.2 `report facets`):

1. Requirement criteria stay surface-independent outcomes and invariants.
   A surface-specific criterion is allowed only when the boundary itself
   carries meaning (for example, an HTTP status code).
2. One semantic capability feature carries the current implementation
   targets that reach the same outcome, one `role: implementation` binding
   per project-defined facet with direct `satisfies` claims.
3. `role: operation` bindings stay routing-only; they never substitute for
   semantic implementation. OpenAPI stays `role: contract-source`.
4. Every new implementation target ships its exact verification claim in
   the same change, covering the target through the test that exercises
   that facet.
5. Mitase never decides which facets are required. A missing facet is a
   product decision, never a validation failure. Never invent a target or
   a claim to fill a blank cell.
6. One exact artifact has one implementation owner. Splitting shared
   transport code into exact helper symbols comes before claiming
   separate `cli-core` / `cli-remote` facets.

Old-style specifications stay valid and migrate on touch. Inspect the
current projection at any time:

```bash
./scripts/mitase report facets FEAT-ENTRY-001 . --format json
./scripts/mitase report facets 'REQ-ENTRY-001#criterion.creation' . --format json
```

## Capability versus Journey responsibilities

A semantic capability feature answers one question: which facets reach
the same meaning. A journey answers a different one: do capabilities
A → B → C compose into a durable end-to-end outcome. Journeys verify
composition through capability-owned targets; they do not re-own surface
targets and they do not own surface completeness.

## Verification and evidence

Cover the operation at the owning layer, then add adapter parity where the
surface changes behavior:

- domain/core tests for validation, persistence, authority, and recovery;
- CLI/server/WASM tests for translation and error boundaries;
- frontend tests for user-owned interaction behavior;
- E2E only for an Ugoite-owned cross-surface or deployment contract;
- Mitase evidence for requirements and policy claims; and
- canonical task/reference documentation with current/future boundaries.

Run `mise run check` and `mise run test` at the repository root. If the change
touches packaged output or container startup, include the artifact and E2E lanes
required by root `mise.toml`.

## Documentation and release boundaries

Write task guidance in `docs/use`, operator procedures in `docs/operate`,
contributor workflow in `docs/develop`, and exact machine facts in `docs/reference`
or their machine authority. Normative requirements and traceability belong in
Specification/Mitase.

Before opening a PR, complete the Knowledge Compatibility Review when the
change can affect Space ownership, current-state authority, publication
reachability, history reconstruction, or adapter authority. Do not update the
published v0.1 line or release metadata as part of ordinary feature work.

## Related

- [Repository Map](repository-map.md)
- [Engineering Principles](engineering-principles.md)
- [Documentation Development](documentation.md)
- [Architecture](../architecture/index.md)
- [Specification](../spec/index.md)
