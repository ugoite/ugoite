---
title: Model Context Protocol v1
---

Ugoite MCP v1 is a small, authenticated semantic facade at `POST /mcp`.
It is stateless and uses MCP protocol revision `2026-07-28`; clients do not
send `initialize`, use `Mcp-Session-Id`, or select a Space by internal ID.

The stable surface is deliberately narrow:

- `ugoite.search` returns five sanitized summaries by default and at most 25
  when requested, with stable `ugoite://entry/{id}` resource links. Entry bodies are
  loaded only when a resource is read.
- `ugoite.save` creates an Entry without `id` or updates one opaque Entry with
  `id`.
- `ugoite.delete` performs only an authorized soft delete.

Tools are filtered before `tools/list`: read-only credentials see search,
write credentials additionally see save, and delete is shown only to an
approved human device credential with effective Delete authority. Tool
descriptions and schemas contain no Form, revision, Iceberg, bucket, path, or
storage details.

Resources are lazy and use stable opaque URIs:

- `ugoite://entry/{id}` — semantic Entry projection;
- `ugoite://entry/{id}/history` — append-only event projection;
- `ugoite://entry/{id}/schema` — the associated Form schema;
- `ugoite://form/{id}` — a semantic Form schema.

`resources/list` is empty and `resources/templates/list` contains only those
four templates. Search and resource results label user-controlled material as
`_untrusted_content: true`, sanitize it, and warn clients never to treat it as
instructions. They never expose storage layout or revision internals.

## Authentication

The protected resource is exactly `{issuer}/mcp`. Protected-resource metadata
is published at `/.well-known/oauth-protected-resource`; authorization-server
metadata is published at `/.well-known/oauth-authorization-server`. Human MCP
credentials are short-lived opaque Bearer credentials bound to issuer, node,
resource, Space, actions, credential generation, and the human device. An MCP
client requests a credential with `POST /oauth/device/authorization` using
`resource: {issuer}/mcp`, the requested Space/actions, and its public key; the
signed-in owner approves the displayed request at `/device`; the client
exchanges the device code at `/oauth/token` with its client assertion and the
same resource. Existing DPoP credentials may use `Authorization: DPoP` plus
one RFC 9449 proof. Agent credentials cannot use the MCP delete tool.

Cookie sessions are rejected at `/mcp`. For v0.1, the supported boundary is the
authenticated MCP protocol, its resource-bound credential flow, and ACL
behavior. The `/device` approval page accepts only MCP-scoped requests; CLI
credential storage, CLI device authorization, and agent credential flows remain
future scope. The MCP adapter never asks for a Space ID, bucket, database, or
filesystem path.

The REST implementation remains `crates/ugoite-server`; `/openapi.json` is the
REST API source of truth. MCP JSON-RPC is intentionally not represented as a
one-to-one REST endpoint or as a storage-specific OpenAPI path.

This surface follows the design rule: breadth is cheap, depth is lazy. New
Ugoite capabilities should first be expressed through search and semantic
resources; a new MCP tool is a versioned exception, not the default result of
adding an internal service operation.

Each authenticated `tools/call` credential is limited to 60 calls per rolling
minute. Excess calls receive HTTP 429 with a JSON-RPC rate-limit error and
`Retry-After`; read/list operations are not charged against this limit.
