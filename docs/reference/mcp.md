---
title: "MCP"
description: The small authenticated semantic facade.
sidebar:
  order: 4
---

Ugoite MCP v1 is a small, authenticated semantic facade at `POST /mcp`. It is
stateless and deliberately narrow: search, save, undo, and authorized soft
delete over the same Space-owned Knowledge.

## Start here

Read the [MCP surface](../architecture/api/mcp.md) for the stable tool and
resource inventory, including `ugoite.search` summaries with
`ugoite://entry/{id}` links, `ugoite.save` canonicalization, `ugoite.undo`
through Run semantics, and protected-resource metadata with DPoP.

## Boundaries

MCP evolution must not change Space ownership, Catalog authority, or append-only
history. A storage encoding change must not silently change an MCP operation.
MCP and REST credentials cannot cross-use.

## Related

- [REST API and OpenAPI](rest.md)
- [Use Ugoite](../use/index.md) for the same operations without MCP framing.
