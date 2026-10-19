---
title: "Portable API client"
sidebar:
  order: 3
---

`ugoite-api-client` is the shared remote-operation contract for native and
WebAssembly clients.

It defines:

- stable operation names;
- HTTP methods and encoded paths/queries;
- JSON or multipart body intent;
- standard versus development-proxy authentication intent;
- response and error decoding;
- a protocol version and operation manifest.

It does **not** perform network I/O and must remain independent of server
frameworks, async runtimes, browser APIs, storage, and `ugoite-core`.

Native CLI flow:

```text
CLI command -> http::execute -> prepare request -> reqwest transport -> decode response
```

Browser flow:

```text
route/API module -> ugoite-client -> WASM JSON protocol -> JavaScript fetch -> WASM decode
```

The WASM JSON protocol also exposes pure Rust Domain actions. The versioned
`domain.compile_variable_graph.v1` action accepts a strict `VariableGraph` JSON
object in `value` and returns `contract_version: 1`, a deterministic
topological order, and a dependency map. Invalid graphs return the stable
Domain diagnostic codes and context; malformed or unknown fields are rejected
as `invalid_variable_graph_payload`. This action only validates and compiles
the dependency plan. It does not execute SQL or access storage, authorization,
or persistence.

New portable REST operations should be added to the manifest, request
preparation, decoding tests, native adapter, browser adapter, OpenAPI/feature
registry, and end-to-end coverage as applicable.
