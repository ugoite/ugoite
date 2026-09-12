---
title: "Identity and Access"
description: Browser login, CLI device auth, recovery, and membership.
sidebar:
  order: 4
---

Identity separates node-local control state from Space authority. Start with the
[authentication overview](../guide/operate/auth/auth-overview.md), then the
[service-account and agent reference
(future)](../guide/operate/auth/service-accounts.md) for scope boundaries.

## Supported in the current boundary

Passkey and WebAuthn login with opaque sessions, owner-approved Space access
recovery, Remote CLI device authorization with DPoP-bound credentials,
recovery-code plus recovery-only TOTP Account Self-Recovery, Space membership
and ACL enforcement, authenticated MCP access, authorized audit reads, and
invitation-gated OIDC authentication and account linking.

## Future scope

Administrator recovery and agent or service-account principals remain future
scope. TOTP is recovery-only and is not a normal login method.

## What became durable?

Space membership and ACL state. Sessions, device grants, and login challenges
are node-local control state.

## Related

- [Authentication overview](../guide/operate/auth/auth-overview.md)
- [Troubleshooting](troubleshooting.md) for sign-in symptoms.
