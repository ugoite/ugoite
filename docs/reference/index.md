---
title: "Reference"
description: Stable entry points for CLI, REST, MCP, configuration, and compatibility facts.
sidebar:
  label: "Overview"
  order: 1
---

Reference collects machine facts without duplicating them. Each page links to
its authority instead of copying every flag, endpoint, or schema into prose.

## Authorities

- CLI behavior is defined by `ugoite <command> --help`. The narrative
  walkthrough lives in the [CLI guide](../guide/automate/cli.md).
- REST behavior is defined by the server implementation in
  `crates/ugoite-server` and the generated contract at `/openapi.json`. Start
  with the [REST overview](../architecture/api/rest.md).
- MCP behavior is the small authenticated semantic facade described in the
  [MCP surface](../architecture/api/mcp.md).
- Compatibility is defined by the
  [Space compatibility contract](../architecture/contracts/space-compatibility.md)
  and the [release contract](../architecture/release/release-contract.md).
- Platform facts live in
  [platform support](../architecture/release/platform-support.md).

Later steps split this overview into CLI, REST and OpenAPI, MCP, configuration,
and compatibility pages. Until then, the links above remain the stable entry
points.
