# Ugoite frontend

SolidStart browser application for Ugoite, managed through the repository Deno workspace.

## Current runtime

The shipped browser is **server-backed**. `frontend/src/lib/api.ts` reports `mode: "server-backed"`, `browserLocal: false`, and `sync: "none"`. Browser-local Space persistence and optional synchronization are future architecture.

Endpoint semantics come from the Rust `ugoite-api-client` crate through the `ugoite-wasm` JSON protocol. JavaScript owns the runtime `fetch` adapter.

## Commands

From the repository root:

```bash
deno task frontend:dev
deno task frontend:build
deno task frontend:test
```

Or use `mise run dev`, `mise run check`, and `mise run test` for repository-wide workflows. The build first compiles the WASM adapter with `scripts/build-ugoite-wasm.sh`.
