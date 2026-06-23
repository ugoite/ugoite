# Contributing

Ugoite is a Rust-centered, filesystem-first project. Keep behavior in the smallest reusable layer and keep adapters thin.

## Toolchain

The pinned tools are declared in `mise.toml`:

- Rust 1.93.0
- Deno 2.8.3
- `wasm32-unknown-unknown`

```bash
mise run setup
```

## Development loop

```bash
mise run dev
mise run fmt
mise run lint
mise run check
mise run test
```

Run `mise run e2e:smoke` for a representative browser/container path and `mise run e2e` for the full suite.

## Responsibility boundaries

1. Put transport-independent types and validation in `ugoite-domain`.
2. Put filesystem/object-storage mechanics in `ugoite-storage`.
3. Put application behavior in `ugoite-core`.
4. Keep `ugoite-server`, `ugoite-cli`, and `ugoite-wasm` as adapters.
5. Put remote operation names, methods, paths, bodies, auth intent, and decoding in `ugoite-api-client`; it must not perform network I/O.
6. Frontend `*-api.ts` modules call the portable protocol rather than constructing endpoint semantics directly.

The browser is currently server-backed. Do not describe browser-local persistence or synchronization as implemented until code and tests exist.

## API changes

When a REST route changes:

1. update the Rust router and handler;
2. update `ugoite-api-client` when the operation is portable;
3. update adapter and frontend tests;
4. run `cargo run -p xtask -- openapi-generate`;
5. update `docs/spec/features/*.yaml` and human documentation;
6. run `mise run check`.

The generated server document is authoritative; never hand-edit `docs/spec/api/openapi.yaml` without regenerating it.

## Requirements and tests

Requirement IDs use `REQ-<CATEGORY>-<NNN>`. Tests should include the relevant ID in the test name or nearby source so traceability can be generated from the repository. A missing trace is documented as `untraced`, not masked with a deleted path.

## Pull requests

Describe the behavior change, link an issue, list focused validation, and call out changes to storage compatibility, authentication, OpenAPI, or browser/server boundaries. `mise run ci` is the pull-request gate; `mise run ci:merge` is the merge gate.
