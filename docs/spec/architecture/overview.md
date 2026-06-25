# Architecture overview

Ugoite is a layered Rust application around portable Space directories.

```text
frontend / CLI / REST / MCP
          ↓ adapters
ugoite-api-client (remote protocol where applicable)
          ↓
ugoite-core (use cases and authorization-aware service)
          ↓
ugoite-storage + OpenDAL / Iceberg-backed tables
          ↓
operator-owned workspace
```

`ugoite-domain` supplies portable types and validation. `ugoite-wasm` exposes the domain/API protocol to browser JavaScript without owning fetch or persistence.

The current browser is server-backed. The target architecture adds a browser-local runtime and optional synchronization without making the server the mandatory owner of data.
