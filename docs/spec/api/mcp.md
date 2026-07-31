---
title: Model Context Protocol surface
---

The HTTP MCP resource is `GET /mcp/resources/{space_id}/entries/list`,
corresponding to `ugoite://{space_uid}/entries/list`. User content is sanitized
and labeled untrusted.

The node publishes RFC 9728 protected-resource metadata at
`/.well-known/oauth-protected-resource` and authorization-server metadata at
`/.well-known/oauth-authorization-server`. Human MCP clients use device
authorization. Autonomous agents use ES256 client assertions. API requests use
DPoP and a five-minute opaque Ugoite access token whose server-side record is restricted to issuer, node resource,
immutable `space_uid`, actions, principal, credential, and actor chain.

The protected-resource metadata `resource_documentation` value is the absolute
official documentation URL
`https://ugoite.github.io/ugoite/docs/guide/operate/auth/auth-overview/`.
It is intentionally not derived from a node's self-hosted origin.

MCP reads the same core-authorized Entry set as REST, search, and SQL. A token
for another Space, a revoked device/agent, an action outside the token, or a
replayed proof is rejected before resource access.
