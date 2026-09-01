# Repository instructions

## Product intent

Ugoite keeps user-owned Knowledge in portable Space directories. Deployment and storage operators may manage where that Knowledge is hosted, but they do not become its authority. Preserve portability, append-only history, and the ability to use the CLI locally without a mandatory server. The current browser is server-backed; browser-local storage plus optional sync is planned, not shipped.

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

## Specification contract

- Preserve source meaning when migrating Requirements. Missing or ambiguous
  evidence becomes an unverified Criterion or an explicit evidence gap; it
  must not be represented by narrowing the Requirement. Policy levels are
  explicit Ugoite governance decisions, and a Policy claim is `enforces` only
  when a repository validator or required workflow mechanically checks it;
  architectural placement and documented responsibility use `evidences`.
- Mitase validates declared specification relationships and evidence. It does
  not execute Ugoite tests, own repository delivery, or become a second
  Knowledge authority.
