---
title: 'Frontend–server interface'
---

The browser uses `/api` when the server hosts static files or a development proxy is present. The server-generated OpenAPI paths omit that deployment prefix.

Frontend product API modules call the Rust/WASM portable protocol. Rust prepares method/path/query/body/auth metadata and decodes responses; JavaScript performs `fetch`, forwards SSR credentials, manages cookies, and tracks loading state.

Contracts:

- JSON uses server/OpenAPI field names at the transport boundary.
- Authentication failures use `401`; known identities without permission use `403`.
- optimistic Entry updates use revision IDs and may return `409` conflicts.
- browser routes must not bypass Space authorization.
- current runtime capabilities are `server-backed`, `browserLocal=false`, and `sync=none`.
