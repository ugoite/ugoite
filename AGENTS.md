# Repository instructions

## Product intent

Ugoite keeps operator-owned Space directories as the source of truth. Preserve portability, append-only history, and the ability to use the CLI locally without a mandatory server. The current browser is server-backed; browser-local storage plus optional sync is planned, not shipped.

## Architecture

- `ugoite-domain`: pure domain types/validation.
- `ugoite-api-client`: transport-neutral remote operation protocol; no fetch/reqwest/runtime dependencies.
- `ugoite-storage`: storage abstraction and filesystem/object-store mechanics.
- `ugoite-core`: application behavior.
- `ugoite-server`, `ugoite-cli`, `ugoite-wasm`: thin adapters.
- `frontend`: SolidStart UI using the portable protocol.
- `docsite`: Astro docs.

Do not reintroduce a second application implementation in another language or duplicate REST semantics across adapters.

## Commands

```bash
mise run setup
mise run fmt
mise run lint
mise run check
mise run test
mise run e2e:smoke
```

Only root tasks in `mise.toml` are valid. Use Deno tasks for frontend/docsite/e2e work.

## Documentation contract

Treat `crates/ugoite-server` as the REST implementation and `/openapi.json` as the API source of truth. Mark future architecture explicitly. Do not advertise service-account/audit CRUD, passkey/TOTP login, browser-local persistence, or remote CLI asset upload as implemented.

## Specification migration contract

- `docs/mitase` is authoritative for domains that have been migrated into the
  canonical Mitase graph.
- Unmigrated domains remain authoritative in their existing `docs/spec`
  records. For migrated domains, the legacy Foundation, Policy, Requirement,
  and Feature files are immutable read-only migration snapshots; the index
  links the canonical files instead. Maintainers review each migration unit
  against its legacy snapshot before changing authority.
- Policy downstream links that still target unmigrated domains are preserved
  as typed deferred relations in `docs/mitase-migration/policy-edges.yaml`.
  They must not be dropped or replaced by invented partial Mitase nodes.
- Every migrated Policy has an explicit Ugoite-owned normative decision in
  `docs/mitase-migration/policy-levels.yaml`. A Policy claim is `enforces` only
  when a repository validator or required workflow mechanically checks it;
  architectural placement and documented responsibility use `evidences`.
- Preserve source meaning when migrating Requirements. Missing or ambiguous
  evidence becomes an unverified Criterion or an explicit migration gap; it
  must not be represented by narrowing the Requirement.
- For migrated Search, Entry, and Form domains, implementation-affecting
  changes must update the canonical Requirement/Feature representation and
  record exact evidence or an explicit evidence gap. This is the staged
  domain ratchet while the global owned-change gate remains relaxed.
- Mitase validates declared specification relationships and evidence. It does
  not execute Ugoite tests, own repository delivery, or become a second
  Knowledge authority.
