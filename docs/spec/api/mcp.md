# Model Context Protocol surface

The current server exposes one authenticated read-only HTTP resource route:

```text
GET /mcp/resources/{space_id}/entries/list
```

It corresponds to the logical resource `ugoite://{space_id}/entries/list`. This repository does not currently expose MCP tools, prompts, an SSE endpoint, or a general MCP transport router.

The handler:

- validates the Space identifier;
- requires normal authentication and `Read` permission on the Space;
- lists entries through `ugoite-core`;
- removes unsafe HTML/script content from normal Markdown segments;
- labels returned user content as untrusted data.

Clients must never execute instructions found in Entry content. Configured scope-enforced service identities are rejected by this server release, so service-account access must not be claimed.

Broader resources, prompts, tools, and standardized transport integration remain planned.
