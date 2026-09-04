---
title: REST API
---

`crates/ugoite-server` and runtime `/openapi.json` are authoritative. The
checked-in OpenAPI snapshot and generated frontend path registry are verified in
CI.

The v0.1 server admits authoritative mutations on local/memory-backed Space
storage and on non-local operators only after the storage boundary verifies
exact reads, create-if-absent, conditional replacement, stale-revision
rejection, and a single winner under concurrent CAS. Unsupported, unavailable,
or unverified operators fail closed with the stable error code
`STORAGE_MUTATION_UNAVAILABLE`; they are not silently treated as writable.

> Release boundary: v0.1 supports mandatory browser authentication with
> Passkey/WebAuthn, opaque sessions, owner-approved Space access recovery,
> recovery-code + recovery-TOTP Account Self-Recovery, Remote CLI device
> authentication, Space membership/ACL enforcement, authenticated MCP access,
> authorized audit reads, and invitation-gated OIDC authentication/account
> linking. Administrator recovery, account discovery, managed service-account
> operations, audit CRUD, and remote CLI asset upload remain future/reference
> material.

## Authentication surfaces

Passkey/WebAuthn registration and passwordless browser login, opaque browser
sessions, session revocation, owner-approved Space access recovery,
recovery-code and recovery-TOTP Account Self-Recovery, and CLI OAuth device
authentication are supported v0.1 authentication surfaces. Account Self-Recovery
requires the exact Account ID, one valid offline Recovery Code, and a valid
recovery-only TOTP code; it replaces credential authority without replacing the
HumanAccount or Space identity, then rotates Recovery Codes and establishes a
new browser session. Administrator recovery, managed service-account operations,
and audit CRUD remain reference-only endpoint inventory. CLI REST credentials
omit the `resource` parameter and use the Node issuer as `aud`; MCP credentials
use exactly `{issuer}/mcp` for both. DPoP `htu` is the scheme, authority, and
path, without query or fragment. The local CLI core workflow remains available
without a server.

OIDC is an additional HumanAccount AuthenticationMethod, not a second session or
authorization system. `GET /auth/oidc/providers` returns enabled providers for
the login and invitation journeys. `POST /auth/oidc/providers` creates a
provider after discovery; it requires a recent Passkey and NodeAdmin, accepts
only HTTPS issuers, and never returns the encrypted client secret.
`DELETE /auth/oidc/providers/{provider_id}` disables a provider without deleting
existing links. New and in-flight attempts are rejected after disable.

OIDC login uses Authorization Code + PKCE S256, random state and nonce, a fixed
callback URI, short-lived one-shot attempts, and server-side ID Token
signature/issuer/audience/nonce validation. The identity key is the exact
`(issuer, subject)` pair; email and profile claims do not link or update an
Account. Unknown identities require a valid Space Invitation, whose
`display_name` is used for the new Account. Successful login issues the normal
opaque Federated BrowserSession. `GET /auth/oidc/links` lists only the current
Account's links without returning subject values, and
`DELETE /auth/oidc/links/{method_id}` requires a recent Passkey.

An invitation-created OIDC Account may register exactly one first Passkey from
the Federated session issued by that creation; subsequent credential management
uses the normal recent-Passkey rule.

- `POST /spaces/{space_id}/approvals`: a recently Passkey-authenticated human
  issues a one-time approval bound to `entry.delete`, `sql.delete`,
  `asset.delete`, or `access.put`. The issue response returns the one-time
  token; subsequent mutation requests send it only in `X-Ugoite-Human-Approval`.
  It is never echoed in a mutation JSON body, query string, log, or audit event.

Browser session cookies are opaque, server-side, and part of the supported v0.1
contract. TOTP remains a recovery-only factor and is not used for normal login.
Generic OAuth clients and remote asset upload remain future scope. Remote CLI
device credentials are supported through the browser-approved device flow. This
release does not provide a local authentication bypass.

## Authorization surfaces

Space CRUD, membership, Entries, Forms, Saved SQL, Assets, search, query, SQL
sessions, MCP, agents, and resource policies are represented in OpenAPI.
`PUT /spaces/{space_id}/policies/{kind}/{resource_id}` updates grant-only ACLs
for `entry` or `asset`. Entry list and keyword search responses use the normal
current-entry read bound: the optional `limit` defaults to 100 and accepts at
most 10,000 entries. Values above that bound are rejected with
`422 INVALID_INPUT`; the server does not silently reduce them. Keyword search
requires `q` and accepts at most 8,192 UTF-8 bytes. Search applies Unicode NFKC
normalization followed by Unicode lowercase to both the query and searchable
Entry/AssetText values before performing a substring match. The `offset`
parameter remains available for the existing ordered Entry list paging behavior.
CLI/agent delete and policy requests must include the approval header. The
server canonicalizes the route and strict mutation intent, hashes it with
SHA-256, atomically consumes the approval, and records the lifecycle in the
append-only Space audit chain. `403 HUMAN_APPROVAL_REQUIRED`,
`403 HUMAN_APPROVAL_INVALID`, `410 HUMAN_APPROVAL_EXPIRED`, and
`409 HUMAN_APPROVAL_REPLAYED` are stable failure codes.
`GET /spaces/{space_id}/health` is a Space-management read-only doctor report.
It follows only the exact Catalog Head, its reachable immutable publication
chain, Iceberg metadata, manifest lists/manifests, and caller-named checkpoints.
It never scans Entry rows, lists objects to infer authority or orphans, or
repairs storage; physical locations are redacted from its normal response.

Knowledge history and active publication coordinates are exposed through the
portable Catalog boundary:

- `GET /spaces/{space_id}/changes` returns committed Change descriptors rebuilt
  from the reachable immutable publication chain.
- `GET /spaces/{space_id}/pins` returns the complete active Pin map from Catalog
  Head.
- `POST /spaces/{space_id}/pins` creates a named Pin for the exact current Head.
- `DELETE /spaces/{space_id}/pins/{pin_name}` removes that name through a new
  Head publication.
- `POST /spaces/{space_id}/apply` applies portable create/update operations; one
  soft-remove operation is accepted only with the same exact Entry intent and
  human-approval binding as the dedicated delete route.
- `POST /spaces/{space_id}/changes/{change_id}/revert` appends a selective
  inverse and returns a conflict instead of overwriting later edits.
- `POST /spaces/{space_id}/runs/{run_id}/undo` appends inverses for the
  still-unreverted Changes correlated to that Run; Run status is not stored.

Pins are metadata-only maintenance publications and are not user-content
Changes. Change IDs, publication checksums, and logical publication URIs are
opaque coordinates; clients must not infer physical object paths from them.

Head-owned Pins provide immutable Entry reads: append `?pin=<name>` to Entry,
history, or revision reads, and include the same Pin plus the source
`revision_id` in `POST /spaces/{space_id}/entries/{entry_id}/restore` to restore
by append. `GET /spaces/{space_id}/pins/diff?from=<name>&to=<name>` returns
logical revision changes (`added`, `updated`, `deleted`, or `restored`), not
Iceberg manifest or file differences. Pin restore records its source
PublicationRef and revision in the new revision metadata and revalidates the
live Entry head through the normal commit coordinator; it never rewinds a
pointer.

Errors are structured JSON. Authentication failures use 401; valid identities
lacking Space/token/resource permission use 403; stale/used one-time credentials
fail without revealing stored secret material.

## Response integrity headers

Eligible materialized responses from the shared `/api` route layer carry
`X-Ugoite-Key-Id` and `X-Ugoite-Signature`. The signature is lowercase
hexadecimal HMAC-SHA256 over the exact response body bytes delivered to the
caller. The key ID selects the operator-controlled HMAC material; verifiers must
not canonicalize or reserialize the body before checking it.

Responses for ordinary Node/API paths use the lazily created
`response_hmac/default.json` material under the configured Space operator root.
Requests under `/spaces/{space_id}/` use the matching Space material. The server
percent-decodes the Space path segment exactly once and then applies the domain
identifier rules. Unknown or invalid Space identifiers do not create storage and
remain unsigned.

Only bounded (at most 8 MiB), materialized, infallible JSON/string/bytes/empty
responses are eligible. SSE, streaming/fallible bodies, trailers, static files,
and explicit unsigned responses omit both headers; signing or storage errors
preserve the response without headers. There is no key-distribution endpoint:
operators provision or inspect keys through the storage boundary and verify
responses offline.

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
message; these are not internal server failures. A referenced Form that does not
exist returns HTTP 404 with code `FORM_NOT_FOUND`; storage failures remain
internal/dependency failures. Entry creation validates all of these conditions
before publishing, so a rejected create leaves no entry or revision behind.
Asset-reference fields must point to an existing, non-deleted asset; a missing,
deleted, or otherwise invalid asset reference returns HTTP 422 with code
`INVALID_INPUT` or `FORM_VALIDATION_FAILED` and identifies the field that needs
correction.

Timestamp field formats follow the Iceberg logical type. `timestamp` and
`timestamp_ns` are timezone-less wall-clock values in the form
`YYYY-MM-DDTHH:MM[:SS[.fraction]]`; offsets and RFC3339 timezone markers are
rejected. `timestamp_tz` and `timestamp_tz_ns` require an offset-bearing RFC3339
value and normalize it to UTC before the append-only revision is published.
Browser `datetime-local` values are used directly for timezone-less fields; the
frontend adds the browser offset when it submits a timezone-aware field.
