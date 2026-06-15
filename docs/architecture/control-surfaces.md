# Control Surfaces

Ugoite currently exposes two supported control surfaces.

## Browser

The browser app is server-backed. UI code calls the frontend client boundary,
which targets the Rust `ugoite-server` HTTP API. The server decodes HTTP
requests, applies auth/session policy, maps errors to HTTP responses, and calls
the `ugoite-core` service boundary.

## CLI

The CLI defaults to direct core mode. In that mode, commands call the same
`ugoite-core` service boundary over local or configured OpenDAL storage. The CLI
can also be configured for backend/API mode, where it acts as a remote adapter
over the HTTP surface.

## MCP

The current MCP surface is resource-first and read-oriented. It is exposed by
`ugoite-server` as an adapter over the same core data model. Tool-style MCP
workflows remain future scope.
