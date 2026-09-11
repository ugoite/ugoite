---
title: "Current Product and Target State"
description: What is current, what is next, what is North Star, and what is not promised.
sidebar:
  order: 4
---

This page keeps four time horizons apart. Current documentation describes what
ships. Planned pages carry a Planned badge. North Star describes direction.
Not-promised work is named so it cannot be mistaken for a roadmap commitment.

## Current

The v0.1 release establishes the authority boundary for operator-owned Spaces:

- CLI core mode directly opens a local workspace and is the minimal local-first
  path.
- The Rust server exposes authenticated REST, the small authenticated MCP
  semantic facade, and static browser hosting.
- The browser is server-backed and requires the Rust server. Passkey/WebAuthn
  login, opaque sessions, owner-approved Space access recovery, Remote CLI
  device credentials, recovery-code plus recovery-only TOTP Account
  Self-Recovery, Space membership and ACL enforcement, authenticated MCP access,
  authorized audit reads, and invitation-gated OIDC authentication and account
  linking are supported within the v0.1 boundary.
- Revisions and publication history are append-only; current state is derived
  and recovery never depends on a hidden database.

Administrator recovery, agent and service-account principals, generic OAuth
client compatibility, and audit CRUD remain outside the supported v0.1 contract.
TOTP is recovery-only and is not a normal login method.

## Next

The next direction is v0.2 Product UX: make the frozen v0.1 Foundation
completable, discoverable, and consistent across surfaces through completion,
discoverability, cross-surface consistency, validation clarity, recovery, and
documentation correctness.

Knowledge-to-tools remains a North Star during v0.2, not a shipped acceptance
claim. Former View and AI milestone authorities are obsolete and are no longer
active v0.2 scope.

## North Star

Humans and agents compose portable, inspectable Views and task-specific tools
from the same Space-owned Knowledge. Durable definitions, when introduced,
remain Space content; rendered and execution state remains runtime state.

The target does not require arbitrary code execution, arbitrary package
installation, an app-specific backend or database, hidden durable application
state, or an application-specific authorization authority.

## Not promised

Browser-local persistence and optional synchronization, View and Application
definitions, renderers, low-code composition, a general application builder, and
an arbitrary code runtime are not shipped. No general application builder or
arbitrary code runtime is claimed as current.

See [The Ugoite Vision](index.md) for the promise and
[Design History](design-history.md) for why Vision stays separate from
implementation detail.
