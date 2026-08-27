---
title: Ugoite
description: A private, portable knowledge space you can run with Docker, automate from the CLI, and keep on infrastructure you control.
hero:
  tagline: A private, portable knowledge space built around operator-owned files.
  actions:
    - text: Container quick start
      link: docs/guide/start/container-quickstart/
      icon: right-arrow
    - text: Run from source
      link: docs/guide/develop/local-dev-auth-login/
      variant: minimal
    - text: View on GitHub
      link: https://github.com/ugoite/ugoite
      icon: external
      variant: minimal
sidebar:
  order: 1
---

Ugoite keeps authoritative entries and revisions in portable Space directories.
Search indexes, SQL sessions, and other acceleration structures are derived data
that can be rebuilt.

## Choose a path

- **Operate it:** start with the
  [container quick start](guide/start/container-quickstart.md), then review
  [operations](guide/operate/server/operations.md).
- **Develop it:** follow the
  [local development login guide](guide/develop/local-dev-auth-login.md) and the
  [architecture overview](spec/architecture/overview.md).
- **Automate it:** use the [CLI guide](guide/automate/cli.md),
  [REST API](spec/api/rest.md), or current [MCP surface](spec/api/mcp.md).
- **Verify it:** browse the [executable specification](spec/index.md), whose
  registries point back to implementation and tests.

## Current product boundary

:::caution[Browser caveat today] The browser application is server-backed and
requires the Rust server. Browser-local persistence and optional synchronization
remain planned work. :::

- CLI core mode directly opens a local workspace and is the minimal local-first
  path.
- The server exposes REST, the small authenticated MCP semantic facade, and
  static browser hosting.
- v0.1 supports mandatory browser authentication with Passkey/WebAuthn,
  opaque sessions, owner-approved Space access recovery, Remote CLI device/DPoP
  credentials, recovery-code + recovery-TOTP Account Self-Recovery, Space
  membership/ACL enforcement, authenticated MCP access, and authorized audit
  reads.
- OIDC linking, administrator recovery, agent/service-account flows, generic
  OAuth client compatibility, audit CRUD, and remote CLI asset upload remain
  outside the supported v0.1 release contract. TOTP is recovery-only.

## Source-of-truth rules

1. Product and engineering prose lives under `docs/` and is rendered directly by
   Starlight.
2. Runtime behavior is authoritative in the Rust and frontend implementation;
   specs link to those source and test paths.
3. The server-generated OpenAPI document is authoritative; the checked-in
   snapshot is generated and drift-checked.
4. `README.md` is an entry point, not a second manual.
