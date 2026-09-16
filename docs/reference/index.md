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

- CLI behavior: the current binary via `ugoite <command> --help` (Clap),
  with the mode and task context in the [CLI reference](cli.md).
- REST behavior: `crates/ugoite-server` and the server-generated
  `/openapi.json`, introduced in the [REST overview](../architecture/api/rest.md).
- Requirements and evidence: Mitase declarations under
  [Architecture & Specification](../spec/index.md).
- Docs explain; they do not duplicate protocol semantics or create a second
  protocol registry.
- MCP behavior: the [MCP surface](../architecture/api/mcp.md).
