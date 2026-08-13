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
- Agents cannot manage members, owners, or agents. An active human with the
  current permission can issue a single-use human approval for the exact `entry.delete`, `sql.delete`,
  `asset.delete`, or `access.put` intent. The approval is bound to the action,
  canonical resource, actor credential, lifecycle epochs, and a 1–300 second
  expiry; the server stores only its SHA-256 hash and consumes it with a
  compare-and-swap. Replay, mismatch, or expiry fails closed and the lifecycle
  is recorded in the append-only Space audit chain. Member, owner, agent, and
  Space-management operations remain Passkey-only.
- Fixed passwords, shared credentials, preconfigured bearer credentials,
  development bypasses, default credentials, and long-lived universal tokens do
  not exist.

`UGOITE_PUBLIC_ORIGIN` must be HTTPS except on loopback. The WebAuthn RP ID must
be a registrable suffix of that origin host and must be configured before
credential registration.

## Credentialed CORS policy

CORS is off by default. Setting `UGOITE_CORS_ALLOWED_ORIGINS` enables
credentialed browser access only for the exact origins in its comma-separated
allowlist. Preflight responses advertise the server's explicit method and
request-header allowlists, including the MCP protocol headers for `/mcp`;
wildcard origins, methods, and request headers are not used with credentials.
An origin outside the allowlist does not receive
`Access-Control-Allow-Origin`, so it is not granted browser CORS access.

CORS response permission is independent of CSRF protection. Cookie-authenticated
unsafe requests still require the canonical `UGOITE_PUBLIC_ORIGIN`, even when a
different origin is present in `UGOITE_CORS_ALLOWED_ORIGINS`.

## HTTP response security policy

`ugoite-server` adds the following headers to every response, including static
browser responses, metadata, API success and error responses, CORS preflight
responses, and middleware-generated responses:

- `Content-Security-Policy` restricts scripts and connections to the same
  origin, disallows framing and plugins, permits the static browser's
  same-origin WebAssembly and external manifest boot script, and permits
  `blob:` previews for user-selected assets through the image, frame, and media
  source policies. The script policy has no `unsafe-inline`; the limited CSS
  inline-style exception supports the existing interactive layout.
- `X-Content-Type-Options: nosniff`
- `X-Frame-Options: DENY`
- `Referrer-Policy: strict-origin-when-cross-origin`
- `Permissions-Policy: camera=(), microphone=(), geolocation=()`

`Strict-Transport-Security: max-age=31536000; includeSubDomains` is added only
when `UGOITE_PUBLIC_ORIGIN` is HTTPS on a non-loopback host. HTTP origins and
localhost, IPv4-loopback, and IPv6-loopback HTTPS origins intentionally do not
receive HSTS, so local development does not create browser state that assumes
TLS.

## API response HMAC signing

Eligible materialized responses from the shared `/api` route layer are signed
over the exact bytes delivered in the HTTP body. The server returns:

- `X-Ugoite-Key-Id`: the active scope's HMAC key identifier;
- `X-Ugoite-Signature`: lowercase hexadecimal HMAC-SHA256 over the delivered
  body bytes.

The default Node response scope uses `response_hmac/default.json` below the
configured Space operator root. A valid `/spaces/{space_id}/...` request uses
the corresponding `spaces/{space_id}/hmac.json`; the URI segment is
percent-decoded exactly once before domain validation. The stateless `/mcp`
facade is a separate semantic JSON-RPC surface and is not response-signed by
the REST HMAC layer. The default key is Node-local and is not part of a Space
export. Key material is intentionally not distributed by an HTTP endpoint;
operators and offline verifiers provision or read it through their storage
boundary.

The shared API marker signs only bounded (at most 8 MiB), materialized,
infallible JSON/string/bytes/empty responses, including authentication and
middleware-generated responses within that API layer. SSE, streaming or
fallible bodies, trailer-bearing responses, static-file responses, and
responses explicitly marked unsigned omit both HMAC headers. A signing or key
storage failure also leaves the original response unsigned and does not expose
secret material.
