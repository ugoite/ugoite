---
title: Security architecture
---

Authentication and authorization follow
[the operator guide](../../guide/auth-overview.md). The normative invariants
are:

- Passkey/WebAuthn is the standard human authenticator; OIDC
  authorization-code + PKCE is an optional linked method keyed by issuer and
  subject.
- Browser cookies contain only an opaque server-side session identifier. Idle
  timeout is 24 hours and absolute timeout is 30 days.
- Setup stays uninitialized until two Passkeys or Passkey + TOTP + recovery
  codes are established. Setup and invitations are random, expiring, one-use values stored only as
  SHA-256 hashes.
- Recovery codes are one-use hashes. Replacing a Passkey requires a recovery
  code plus TOTP; the TOTP seed is encrypted at rest and TOTP alone is never
  sufficient.
- Node administrator and Space owner are separate roles.
- `space_uid`, principals, memberships, ACLs, attribution, and authorization
  audit events are portable Space state; accounts, bindings, sessions, RP
  configuration, and credential metadata are Node-local atomic control-store state.
- CLI and agents register P-256 public keys. Access tokens are opaque,
  hash-stored, five-minute,
  Space/action restricted, and sender-constrained with RFC 9449 DPoP.
- Entry and Asset use grant-only ACLs with default Space-role inheritance. Every
  adapter uses the core Authorizer. SQL
  constructs authorized source tables before joins, counts, and aggregates.
- The last Space owner and last account Passkey cannot be removed.
- Agents cannot manage members, owners, or agents. Noninteractive credentials
  cannot perform delete or share.
- Fixed passwords, shared credentials, preconfigured bearer credentials, development
  bypasses, default credentials, and long-lived universal tokens do not exist.

`UGOITE_PUBLIC_ORIGIN` must be HTTPS except on loopback. The WebAuthn RP ID must
be a registrable suffix of that origin host and must be configured before
credential registration.
