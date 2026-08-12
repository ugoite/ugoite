# Ugoite frontend

SolidStart browser application for Ugoite, managed through the repository Deno workspace.

## Current runtime

The shipped browser is **server-backed**. `frontend/src/lib/api.ts` reports `mode: "server-backed"`, `browserLocal: false`, and `sync: "none"`. Browser-local Space persistence and optional synchronization are future architecture.

Endpoint semantics come from the Rust `ugoite-api-client` crate through the `ugoite-wasm` JSON protocol. JavaScript owns the runtime `fetch` adapter.

## Commands

From the repository root:

```bash
mise run build:wasm
mise run build:frontend
mise run test:frontend
mise run test:frontend:coverage
```

Package-local Deno tasks are low-level commands. The supported orchestration surface is the root Mise task graph, which activates the correct WASM output before frontend build and test runs.
