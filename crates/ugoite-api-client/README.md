# ugoite-api-client

`ugoite-api-client` is Ugoite's transport-neutral remote API protocol layer.
It is compiled natively into `ugoite-cli` and compiled to WebAssembly through
`ugoite-wasm` for the frontend.

It owns the pieces that must not drift between clients:

- stable operation names
- HTTP methods
- percent-encoded path segments and query parameters
- JSON request serialization
- authentication intent (`standard` or `dev_proxy`)
- successful response decoding
- structured API error decoding
- detection of HTML returned from a misconfigured API base

It deliberately does **not** own network I/O, cookies, TLS, SSR request context,
loading state, storage, or business workflows.

```text
ugoite-cli --reqwest--> ugoite-api-client
frontend --fetch--> ugoite-wasm --> ugoite-api-client
```

## Dependency rule

This crate must remain usable on `wasm32-unknown-unknown`. Do not add dependencies
on `reqwest`, `tokio`, `axum`, `web-sys`, `wasm-bindgen`, `ugoite-core`,
`ugoite-storage`, OpenDAL, Iceberg, Arrow, or Parquet.

Run the focused structural guard from the repository root:

```bash
deno run -A scripts/check-portable-api-client.ts
```

Run the native tests with:

```bash
cargo test -p ugoite-api-client --locked
```

The complete change and extension procedure is documented in
[`../../IMPLEMENTATION_PLAN.md`](../../IMPLEMENTATION_PLAN.md).
