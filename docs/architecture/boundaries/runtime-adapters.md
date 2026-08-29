---
title: "Runtime adapters"
sidebar:
  order: 2
---

Runtime-specific code surrounds the portable Rust model.

## Native server

Axum maps HTTP requests to authenticated identities, checks Space permissions,
invokes `UgoiteService`, and converts domain/service errors into HTTP responses.
Static browser files may be served from `UGOITE_STATIC_DIR`.

## Native CLI

Core mode constructs `UgoiteService` against a local root. Backend/API mode uses
`ugoite-api-client` request preparation and a native HTTP transport. Local-only
index maintenance is intentionally unavailable in remote mode. One-shot core
Entry/Form/Asset mutations return after the authoritative commit; their
process-local Derived refresh is best effort and is never drained at command
exit. `ugoite index run` is the explicit freshness repair command.

## Browser

JavaScript owns `fetch`, cookies, UI state, and route navigation. WASM owns
portable request/response protocol logic. The current browser has no complete
local Space storage adapter.

## Konase

ugoite-konase is a portable client-side control plane used by native and WASM
adapters. Its deterministic step function returns serializable host effects
and does not perform I/O. Konase state and bounded observations are disposable;
durable Knowledge remains in the existing Space and Change/Run/Undo boundaries.
The browser Host and native CLI each execute one Job at a time and discard
their runtime state after completion; the browser has no persistent chat or
browser-local Space store.

## Storage

`ugoite-storage` supplies the operator abstraction used by core. Storage
configuration is an operator concern; adapters must not create another
authoritative database.
