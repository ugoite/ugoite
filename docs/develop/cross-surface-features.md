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
