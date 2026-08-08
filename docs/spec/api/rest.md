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
- `POST /auth/invitations/start|finish`: invited Passkey registration. The
  start response can request a retry after a previous registration claim; the
  browser must complete normal Passkey login and then use authenticated
  acceptance to converge the Node binding and Space membership. The invitation
  token alone never creates a session or authenticates an account.
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
`GET /spaces/{space_id}/health` is a Space-management read-only doctor report.
It follows only the exact Catalog Head, its reachable immutable publication
chain, Iceberg metadata, manifest lists/manifests, and caller-named
checkpoints. It never scans Entry rows, lists objects to infer authority or
orphans, or repairs storage; physical locations are redacted from its normal
response.

Named checkpoints also provide immutable Entry reads: append `?checkpoint=<name>`
to Entry, history, or revision reads, and include the same checkpoint plus the
source `revision_id` in `POST /spaces/{space_id}/entries/{entry_id}/restore` to
restore by append. `GET /spaces/{space_id}/checkpoints/diff?from=<name>&to=<name>`
returns logical revision changes (`added`, `updated`, `deleted`, or `restored`),
not Iceberg manifest or file differences. Checkpoint restore records its source
checkpoint and revision in the new revision metadata and revalidates the live
Entry head through the normal commit coordinator; it never rewinds a pointer.

Errors are structured JSON. Authentication failures use 401; valid identities
lacking Space/token/resource permission use 403; stale/used one-time credentials
fail without revealing stored secret material.

Form evolution that changes the type of an existing field is intentionally
unsupported before v1. The server returns HTTP 422 with code
`FORM_FIELD_TYPE_CHANGE_NOT_SUPPORTED` and a message naming the field and
recommending a new field; it does not return this expected rejection as a 500.
Removing an existing physical field is also unsupported before v1. The server
returns HTTP 422 with `FORM_FIELD_REMOVAL_NOT_SUPPORTED`, naming the field and
directing the caller to add a new field instead. The browser editor disables
these destructive/non-compatible operations; the server remains the authority
for CLI and other protocol callers.

Entry create and update validate Markdown sections against the selected Form.
Invalid field values and missing required fields return HTTP 422 with code
`FORM_VALIDATION_FAILED`; unknown fields return HTTP 422 with code
`UNKNOWN_FORM_FIELDS`. Missing Form metadata and invalid row references return
HTTP 422 with code `INVALID_INPUT`, with the offending field and a corrective
message; these are not internal server failures. A referenced Form that does
not exist returns HTTP 404 with code `FORM_NOT_FOUND`; storage failures remain
internal/dependency failures. Entry creation validates all of these conditions
before publishing, so a rejected create leaves no entry or revision behind.
Asset-reference fields must point to an existing, non-deleted asset; a missing,
deleted, or otherwise invalid asset reference returns HTTP 422 with code
`INVALID_INPUT` or `FORM_VALIDATION_FAILED` and identifies the field that needs
correction.

Timestamp field formats follow the Iceberg logical type. `timestamp` and
`timestamp_ns` are timezone-less wall-clock values in the form
`YYYY-MM-DDTHH:MM[:SS[.fraction]]`; offsets and RFC3339 timezone markers are
rejected. `timestamp_tz` and `timestamp_tz_ns` require an offset-bearing
RFC3339 value and normalize it to UTC before the append-only revision is
published. Browser `datetime-local` values are used directly for timezone-less
fields; the frontend adds the browser offset when it submits a timezone-aware
field.
