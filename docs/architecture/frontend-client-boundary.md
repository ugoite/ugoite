# Frontend Client Boundary

Frontend UI code should depend on the Ugoite client boundary instead of raw
HTTP details. The boundary is exported from `frontend/src/lib/ugoite-client/`
and currently wraps the existing API modules.

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
Components and stores should call the boundary modules; OpenAPI-generated or
raw fetch details stay behind that boundary.
