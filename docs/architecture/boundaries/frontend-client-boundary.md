---
title: "Frontend client boundary"
---

The frontend owns interaction state, rendering, routing, and the runtime HTTP
transport. It does not own endpoint semantics.

`frontend/src/lib/ugoite-client.ts` invokes the JSON protocol compiled from
`ugoite-wasm`. The portable Rust client prepares operation
method/path/query/body/auth metadata and decodes the response. JavaScript
performs `fetch`, supplies browser credentials, and returns the raw response
envelope for Rust decoding.

`frontend/src/lib/*-api.ts` modules expose product-oriented methods to routes
and components. They must not reconstruct REST paths with direct `apiFetch`
calls.

Current runtime capabilities in `frontend/src/lib/api.ts` are:

```text
mode = server-backed
browserLocal = false
sync = none
```

A future browser-local runtime belongs behind a runtime adapter with the same
product-level interface. Documentation must not describe it as implemented
before storage, conflict handling, migration, and tests exist.
