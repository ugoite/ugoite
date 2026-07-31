---
title: "Automate Ugoite"
description: Use the CLI and scoped Agent Principals without turning a Space into a service database.
sidebar:
  label: "Overview"
  order: 1
---

Ugoite supports automation in two deliberately different ways. Core mode lets
the CLI open an operator-owned Space directly. Backend/API mode uses the
portable operation protocol, authentication, and authorization exposed by the
Rust server.

## Choose a client identity

- Use the [CLI guide](cli.md) for local core-mode commands and remote
  server-backed commands.
- Use [Agent identities](../operate/auth/service-accounts.md) for scoped,
  revocable automation with a public key. Shared secrets and long-lived API keys
  are not supported.

The CLI is an adapter. Domain behavior remains in the shared Rust core, so a
local command and a server-backed operation preserve the same Space boundary.
