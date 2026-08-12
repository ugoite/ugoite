---
title: Security architecture
---

Authentication and authorization follow
[the operator guide](../../guide/operate/auth/auth-overview.md). The normative
invariants are:

- Passkey/WebAuthn is the standard human authenticator; OIDC
  authorization-code + PKCE is an optional linked method keyed by issuer and
  subject.
- Browser cookies contain only an opaque server-side session identifier. Idle
  timeout is 24 hours and absolute timeout is 30 days.
- Setup stays uninitialized until two Passkeys or Passkey + TOTP + recovery
  codes are established. Setup and invitations are random, expiring, one-use
  values stored only as SHA-256 hashes.
- Recovery codes are one-use hashes. Replacing a Passkey requires a recovery
  code plus TOTP; the TOTP seed is encrypted at rest and TOTP alone is never
  sufficient.
- Owner-approved recovery is separate from self-recovery: only an active human
  Space Owner with a recent Passkey can issue a 15-minute token for another
  active member. Its hash is authoritative; encrypted response material is
  retained only for a bounded one-time response window. The target completes a
  five-minute WebAuthn flow; credential generation invalidates old human
  credentials while agent credentials remain separate. Backup-code rotation
  uses a UUIDv4 `Idempotency-Key` and returns eight codes once.
- Node administrator and Space owner are separate roles.
- An active human Space Owner with a recent phishing-resistant browser session
  may issue a 15-minute, 256-bit owner-approved recovery token for another
  active human member. The token is one-use, and its encrypted response copy is
  limited to the bounded one-time response boundary; the target completes a
  five-minute WebAuthn replacement flow. A new force-reset request deliberately
  supersedes the previous approval. Reset advances a credential
  generation, invalidating old human sessions and credentials while preserving
  separate agent credentials.
- Owner backup-code rotation requires a fresh UUIDv4 `Idempotency-Key`, returns
  eight plaintext codes once, and advertises `audit_status: pending` when audit
  delivery is queued. No recovery token, code, TOTP secret, or hash is logged.
- Recovery approval and completion bind to durable Node/Space lifecycle epochs
  and a Space-CAS recovery fence. Membership changes conflict while a fence is
  held; a Node-committed but unreconciled result remains a durable
  `node_committed_space_fence_pending` marker and returns
  `409 RECOVERY_FENCE_UNAVAILABLE` until restart reconciliation completes it.
- `space_uid`, principals, memberships, ACLs, attribution, and authorization
  audit events are portable Space state; accounts, bindings, sessions, RP
  configuration, and credential metadata are Node-local atomic control-store
  state.
- CLI and agents register P-256 public keys. Access tokens are opaque,
  hash-stored, five-minute, Space/action restricted, and sender-constrained with
  RFC 9449 DPoP.
- Entry and Asset use grant-only ACLs with default Space-role inheritance. Every
  adapter uses the core Authorizer. SQL constructs authorized source tables
  before joins, counts, and aggregates.
- The last Space owner and last account Passkey cannot be removed.
- Agents cannot manage members, owners, or agents. Noninteractive credentials
  cannot perform delete or share.
- Fixed passwords, shared credentials, preconfigured bearer credentials,
  development bypasses, default credentials, and long-lived universal tokens do
  not exist.

`UGOITE_PUBLIC_ORIGIN` must be HTTPS except on loopback. The WebAuthn RP ID must
be a registrable suffix of that origin host and must be configured before
credential registration.

## HTTP response security policy

`ugoite-server` adds the following headers to every response, including static
browser responses, metadata, API success and error responses, CORS preflight
responses, and middleware-generated responses:

- `Content-Security-Policy` restricts scripts and connections to the same
  origin, disallows framing and plugins, permits the static browser's
  same-origin WebAssembly and external manifest boot script, and permits
  `blob:` previews for user-selected assets. The script policy has no
  `unsafe-inline`; the limited CSS inline-style exception supports the
  existing interactive layout.
- `X-Content-Type-Options: nosniff`
- `X-Frame-Options: DENY`
- `Referrer-Policy: strict-origin-when-cross-origin`
- `Permissions-Policy: camera=(), microphone=(), geolocation=()`

`Strict-Transport-Security: max-age=31536000; includeSubDomains` is added only
when `UGOITE_PUBLIC_ORIGIN` is HTTPS on a non-loopback host. HTTP origins and
localhost, IPv4-loopback, and IPv6-loopback HTTPS origins intentionally do not
receive HSTS, so local development does not create browser state that assumes
TLS.
