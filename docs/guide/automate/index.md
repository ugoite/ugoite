---
title: "Automate Ugoite"
description: Use the CLI and understand future scoped automation without turning a Space into a service database.
sidebar:
  label: "Overview"
  order: 1
---

Ugoite supports automation in two deliberately different ways. Core mode lets
the CLI open an operator-owned Space directly. Backend/API mode uses the
portable operation protocol, authentication, and authorization exposed by the
Rust server.

> Release note: Agent/service-account identity workflows described in this
> section are future/reference material and are not supported v0.1 product
> capabilities.

## Choose a client identity

- Use the [CLI guide](cli.md) for local core-mode commands and remote
  server-backed commands.
- Agent identities are documented as a future scoped-automation design; they
  are not a supported v0.1 client workflow.

The CLI is an adapter. Domain behavior remains in the shared Rust core, so a
local command and a server-backed operation preserve the same Space boundary.
