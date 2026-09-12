---
title: "Reference"
description: Stable entry points for CLI, REST, MCP, configuration, and compatibility facts.
sidebar:
  label: "Overview"
  order: 1
---

Reference collects machine facts without duplicating them. Each page links to
its authority instead of copying every flag, endpoint, or schema into prose.

## Pages

- [CLI](cli.md): modes, authentication, output conventions, and command
  families. Full flags live in `ugoite <command> --help`.
- [REST API and OpenAPI](rest.md): auth model, error model, and the
  server-generated contract.
- [MCP](mcp.md): the small authenticated semantic facade.
- [Configuration](configuration.md): environment variables and entry points.
- [Storage and Compatibility](compatibility.md): portable guarantees and
  platform facts.

## Authorities

- CLI behavior: `ugoite <command> --help` with the narrative in the
  [CLI guide](../guide/automate/cli.md).
- REST behavior: `crates/ugoite-server` and `/openapi.json`, introduced in the
  [REST overview](../architecture/api/rest.md).
- MCP behavior: the [MCP surface](../architecture/api/mcp.md).
