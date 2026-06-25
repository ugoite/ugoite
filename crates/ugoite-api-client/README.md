# ugoite-api-client

Portable Ugoite HTTP protocol logic shared by the native CLI and browser/WASM adapter.

It owns stable operation names, HTTP methods, encoded paths and queries, request-body shape, authentication intent, and response/error decoding. It deliberately performs no network I/O. Native code supplies an HTTP transport; JavaScript supplies `fetch` around the WASM protocol.

The public operation manifest is `SUPPORTED_OPERATIONS` in `src/lib.rs`.

```bash
cargo test -p ugoite-api-client --locked
cargo check -p ugoite-api-client --target wasm32-unknown-unknown --locked
```
