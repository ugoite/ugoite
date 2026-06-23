# Portable API Client

Ugoite has two remote API consumers: the native CLI and the frontend. They must
share the Ugoite protocol without forcing either runtime to emulate the other.

## Boundary

```text
ugoite-core ------------------------------------------> ugoite-cli local mode

ugoite-api-client ------------------------------------> ugoite-cli remote mode
(pure protocol logic)                                    reqwest I/O
          \
           +------------------------------------------> ugoite-wasm
ugoite-domain ---------------------------------------->   raw JSON ABI
                                                          |
                                                          v
                                                    frontend fetch
```

`ugoite-domain` and `ugoite-api-client` are portable sibling crates. They do not
depend on each other; `ugoite-wasm` is the facade that exposes both to browser
adapters.

`ugoite-api-client` owns operation semantics: method, encoded path/query, JSON
body, authentication intent, and response/error decoding. It never performs
network I/O.

The CLI owns its native transport concerns:

- `reqwest` and TLS
- bearer token / API key lookup
- local development authentication headers
- remote endpoint safety checks
- connection pooling

The frontend owns its web-runtime transport concerns:

- browser or SSR `fetch`
- relative `/api` versus absolute SSR origins
- incoming cookie and Authorization forwarding during SSR
- loading indicators
- `AbortSignal`
- browser-native `FormData`

`ugoite-wasm` is a thin adapter. It exposes UTF-8 JSON commands over a small raw
WASM ABI and must not depend on `ugoite-core`, `ugoite-storage`, `reqwest`,
`tokio`, or OpenDAL.

## Why fetch stays outside WASM

Browser fetch and native HTTP share protocol semantics but not runtime policy.
Moving `fetch` itself into WASM would make the portable layer aware of SSR
request events, browser cookie restrictions, credentials policy, loading state,
and JavaScript cancellation. Keeping a replaceable transport adapter preserves
the local-first direction and makes browser-local adapters possible later.

## Client command flow

```text
operation name + arguments + optional JSON body
  -> Rust prepare_request
  -> { method, encoded path, headers, body kind, body }
  -> runtime transport
  -> { status, headers, response text }
  -> Rust decode_response
  -> JSON value or normalized ApiProtocolError
```

For multipart uploads, Rust owns the operation path, method, authentication
intent, and response decoding. The runtime owns construction of the multipart
body because `FormData` and native multipart encoders are transport-specific.

## Rules

1. Components and stores import `frontend/src/lib/ugoite-client/`, never raw
   transport details.
2. Frontend `*-api.ts` modules call `protocolFetch`, never `apiFetch` directly.
3. CLI commands call `http::execute` with a stable operation name, never build
   remote URL paths directly.
4. The server remains authoritative for authorization and business validation.
5. Browser-side protocol preparation is a correctness and UX layer, not a
   security boundary.
6. Local CLI/core mode remains independent from this remote protocol path.
7. New operations are added to the Rust manifest, prepare/decode metadata,
   TypeScript manifest, tests, and the implementation-plan operation catalog in
   one change.

The full implementation, validation matrix, and extension checklist are in
[`../../IMPLEMENTATION_PLAN.md`](../../IMPLEMENTATION_PLAN.md).
