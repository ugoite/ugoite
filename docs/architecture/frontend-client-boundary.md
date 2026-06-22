# Frontend Client Boundary

Frontend UI code depends on the Ugoite client boundary instead of raw HTTP
details. The boundary is exported from `frontend/src/lib/ugoite-client/`.
Components and stores must not import endpoint modules or `apiFetch` directly.

The current runtime capabilities are fixed as:

```ts
{
  mode: "server-backed",
  serverBacked: true,
  browserLocal: false,
  sync: "none",
}
```

This makes today's behavior explicit while leaving room for future adapters.

Remote operations pass through `ugoite-client/protocol.ts`. The portable Rust
crate `ugoite-api-client`, compiled through `ugoite-wasm`, owns HTTP methods,
encoded paths and queries, JSON serialization, and response/error semantics.
TypeScript retains only runtime-specific transport policy: browser/SSR fetch,
cookie and Authorization forwarding, loading state, cancellation, and
`FormData`.

See [Portable API Client](portable-api-client.md) for the dependency boundary
and extension rules.
