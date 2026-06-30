---
title: REST API
---

`crates/ugoite-server` and runtime `/openapi.json` are authoritative. The
checked-in OpenAPI snapshot and generated frontend path registry are verified in
CI.

## Authentication surfaces

- `GET /auth/config`: Node lifecycle, issuer, and WebAuthn RP information.
- `POST /auth/setup/start|finish`: one-use first-account Passkey registration;
  the Node remains uninitialized until a second Passkey or confirmed TOTP is added.
- `POST /auth/passkey/start|finish`: discoverable Passkey login and opaque
  session issuance.
- `GET|DELETE /auth/session`: inspect or revoke the current browser session.
- `POST /auth/invitations/start|finish`: invited Passkey registration.
- `GET /auth/oidc/{provider_id}/start` and `GET /auth/oidc/callback`: OIDC
  authorization code + PKCE.
- `GET|POST /auth/oidc/providers`: Node administrator provider configuration.
- `/auth/passkeys` and `/auth/devices`: credential inventory and revocation.
- `/auth/recovery/*`: encrypted TOTP enrollment and recovery-code + TOTP
  replacement Passkey registration.
- `/auth/accounts`: Node administrator account inventory and suspension.
- `POST /oauth/device/authorization`, `/oauth/device/approve`, `/oauth/token`:
  CLI/MCP device flow and rotating refresh.
- `POST /oauth/agent/token`: autonomous agent issuance.

Browser requests authenticate with the `ugoite_session` HttpOnly cookie. CLI,
MCP, and agent requests use `Authorization: DPoP <opaque-access-token>` plus a DPoP
proof header. Only its hash and authorization metadata are stored server-side.
No endpoint accepts external OIDC tokens or preconfigured long-lived
credentials, or setup values as API authorization.

## Authorization surfaces

Space CRUD, membership, Entries, Forms, Saved SQL, Assets, search, query, SQL
sessions, MCP, agents, and resource policies are represented in OpenAPI.
`PUT /spaces/{space_id}/policies/{kind}/{resource_id}` updates grant-only ACLs
for `entry` or `asset`.
`POST /spaces/{space_id}/bindings/rebind-owner` binds an imported Space owner to
the current Node administrator after migration.

Errors are structured JSON. Authentication failures use 401; valid identities
lacking Space/token/resource permission use 403; stale/used one-time credentials
fail without revealing stored secret material.
