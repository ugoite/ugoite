---
title: "Identity and Access"
description: Set up human login, CLI device access, recovery, and Space membership.
sidebar:
  order: 4
---

Identity separates node-local control state from portable Space authority. A
Node administrator operates a deployment; a Space owner manages one Space.
Neither role silently becomes the other.

## Current capability

The current v0.1 boundary supports:

- Passkey/WebAuthn browser login with opaque server-side sessions;
- owner-approved Space access recovery;
- Remote CLI device authorization with DPoP-bound credentials;
- recovery codes plus explicitly enrolled recovery-only TOTP for Account
  Self-Recovery;
- Space membership and ACL enforcement;
- authenticated MCP access and authorized audit reads; and
- invitation-gated OIDC authentication and account linking where configured.

TOTP is recovery-only, not a normal login method. Administrator recovery,
general-purpose agent principals, and service-account automation are not
current product procedures.

## First setup

1. Start the server and open the one-use setup URL from its console or
   container log.
2. Register the initial Passkey and complete the second Passkey ceremony.
3. Save the one-time recovery codes in an owner-controlled location.
4. Sign in again with the registered Passkey and confirm the expected Space
   membership.

The setup secret is short-lived, one-use, and stored only as a hash. Visiting a
server does not grant administrator access. Configure the public origin and
WebAuthn RP ID before this ceremony; see [Configure](configure.md).

## Browser sessions

Browser sessions are opaque server-side Node control records. Restarting with
the same Node control-store prefix and node secret preserves an otherwise valid
session. If either recovery input changes, the browser must sign in again.

Credential enrollment, recovery settings, MCP approval, and membership or role
changes require a recent Passkey. Register more than one Passkey; Ugoite does
not remove the final credential.

## Remote CLI devices

Run `ugoite auth login` for browser-approved device authorization. The CLI
creates a fresh key, shows the target Space and requested actions, and stores
the private key in the OS keychain when available. REST device credentials are
short-lived, revocable, and DPoP sender-constrained.

Use `ugoite auth login --for mcp` for MCP/Konase pairing. REST and MCP
credentials have different targets and cannot cross-use. The [CLI
Reference](../reference/cli.md) explains endpoint modes and points to the
installed command help for exact options.

## Step-up for remote Space mutations

Device tokens can never carry a recent-Passkey ceremony, so remote Space
mutations gated on fresh human presence complete a browser step-up instead:
the CLI starts a short-lived challenge bound to the exact account, credential,
operation, and Space; the browser approves it after its own fresh Passkey
ceremony; the CLI retries the identical mutation once with the challenge.

Step-up fails closed without an existence oracle. Unknown, expired, consumed,
operation-mismatched, and Space-mismatched challenges all return 403
`STEP_UP_INVALID` (no 404 branch). The bound Space is normalized once, so a
padded value can never mismatch the stored binding.

Challenges are single-use without a two-phase commit. Authorization is
evaluated before consumption, so a denied mutation leaves the challenge
consumable; once an authorized mutation attempt consumes it, the challenge
stays consumed even if the later write fails. Request a new challenge before
retrying. The eligible operations are the single canonical set owned by the
identity layer (`space.create`, `space.patch`, `space.members.invite`,
`space.members.update_role`, `space.members.revoke`, `pin.create`,
`pin.delete`).

## Recovery

### Account Self-Recovery

An account that explicitly enrolled recovery protection can use its exact
Account ID, one valid offline recovery code, and a valid recovery-only TOTP to
replace its Passkey. Successful recovery rotates recovery codes, invalidates
old authentication authority, and creates a new session. Neither the code nor
TOTP works alone.

### Owner-approved Space access recovery

An active human Space owner with a recent Passkey can issue a one-use,
short-lived recovery approval for an active member of that Space. The member
completes the WebAuthn ceremony with that approval. The Space Principal ID,
membership, role, ACL, ownership, and audit identity remain unchanged; only the
member's node-local account binding is replaced.

Use the server's REST implementation and `/openapi.json` for exact endpoint
shapes and error details. Never edit identity or authorization files by hand.

## What became durable?

Space principals, memberships, ACLs, attribution, and authorization audit
history are portable Space Knowledge. Accounts, bindings, sessions, device
grants, Passkeys, and login challenges are node-local control state. Preserve
both according to [Storage and Recovery](storage-recovery.md) when restoring a
deployment.

## Future boundary

Agent principals and service accounts may become Space principals with explicit
human sponsorship and grants. They are not shipped login mechanisms, and this
page does not promise them as current automation.

## Related

- [REST API and OpenAPI](../reference/rest.md)
- [Configuration](configure.md)
- [Troubleshooting](troubleshooting.md)
- [Current Product and Target State](../vision/current-and-target.md)
