# Runtime adapters

Runtime-specific code surrounds the portable Rust model.

## Native server

Axum maps HTTP requests to authenticated identities, checks Space permissions, invokes `UgoiteService`, and converts domain/service errors into HTTP responses. Static browser files may be served from `UGOITE_STATIC_DIR`.

## Native CLI

Core mode constructs `UgoiteService` against a local root. Backend/API mode uses `ugoite-api-client` request preparation and a native HTTP transport. Local-only index maintenance is intentionally unavailable in remote mode.

## Browser

JavaScript owns `fetch`, cookies, UI state, and route navigation. WASM owns portable request/response protocol logic. The current browser has no complete local Space storage adapter.

## Storage

`ugoite-storage` supplies the operator abstraction used by core. Storage configuration is an operator concern; adapters must not create another authoritative database.
