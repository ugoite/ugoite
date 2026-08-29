---
title: Ugoite
description: A private, portable Knowledge Space for humans and AI, built around operator-owned Spaces.
hero:
  tagline: Knowledge persists. Work may disappear. Knowledge can become tools.
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

Ugoite is a private, portable Knowledge Space for humans and AI. Knowledge lives
in an operator-owned Space, where it remains recoverable and independent of any
server, browser session, model provider, or generated experience.

The product promise has three parts: durable Knowledge belongs to the operator;
human and agent Work can use that Knowledge without owning it; and the same
Knowledge can eventually become purpose-built tools without being copied into a
second system of record.

## What you can own

Authoritative Entries, Forms, Assets, saved SQL, Changes, and portable history
live in a Space. Search indexes, SQL sessions, and other acceleration structures
are derived data that can be rebuilt. Node accounts and sessions are separate
node-local control state, not a replacement for Space ownership.

## What humans and AI can do

The local CLI, REST, browser, MCP, and Konase use shared Ugoite semantics. The
current Konase implementation provides a client-side Work/Job control plane;
temporary context, model interaction, and execution progress can disappear.
Meaningful results become durable only through the normal Space mutation and
Change/Run/Undo rules.

## Where Ugoite is going

:::note[Target, not a v0.1 feature]
Knowledge can become portable, inspectable views and task-specific tools. The
current v0.1 release establishes the authority boundary only: browser-local
persistence, optional synchronization, View/Application definitions, and
renderers remain future work. No general application builder or arbitrary code
runtime is shipped.
:::

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
  membership/ACL enforcement, authenticated MCP access, authorized audit
  reads, and invitation-gated OIDC authentication/account linking.
- Administrator recovery, agent/service-account flows, generic OAuth client
  compatibility, audit CRUD, and remote CLI asset upload remain
  outside the supported v0.1 release contract. TOTP is recovery-only.

## Source-of-truth rules

1. Product and engineering prose lives under `docs/` and is rendered directly by
   Starlight.
2. Runtime behavior is authoritative in the Rust and frontend implementation;
   specs link to those source and test paths.
3. The server-generated OpenAPI document is authoritative; the checked-in
   snapshot is generated and drift-checked.
4. `README.md` is an entry point, not a second manual.
