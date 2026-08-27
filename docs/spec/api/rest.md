---
title: REST API
---

`crates/ugoite-server` and runtime `/openapi.json` are authoritative. The
checked-in OpenAPI snapshot and generated frontend path registry are verified in
CI.

The server opens each Space through its configured OpenDAL binding. Local
single-writer deployments use `SingleProcess`; non-local bindings are admitted
as `SharedVerified` only after startup or explicit revalidation proves exact
load, create-if-absent, conditional replacement, stale-revision rejection, and
single-winner concurrent CAS. A readable binding whose conditional-write
contract is not proven is `SharedReadOnly`; a binding that cannot provide exact
reads is storage-unavailable. The stable mutation error code is
`STORAGE_MUTATION_UNAVAILABLE`, and the health response exposes the mode and
probe reason.

> Release boundary: v0.1 supports mandatory browser authentication with
> Passkey/WebAuthn, opaque sessions, owner-approved Space access recovery,
> recovery-code + recovery-TOTP Account Self-Recovery,
> Remote CLI device authentication, Space membership/ACL enforcement,
> authenticated MCP access, and authorized audit reads. OIDC linking,
> administrator recovery, account discovery, managed service-account operations,
> audit CRUD, and remote CLI asset upload remain future/reference material.

## Authentication surfaces

Passkey/WebAuthn registration and passwordless browser login, opaque browser
sessions, session revocation, owner-approved Space access recovery, recovery-code
and recovery-TOTP Account Self-Recovery, and CLI OAuth device authentication are
supported v0.1 authentication surfaces. Account Self-Recovery requires the exact
Account ID, one valid offline Recovery Code, and a valid recovery-only TOTP code;
it replaces credential authority without replacing the HumanAccount or Space
identity, then rotates Recovery Codes and establishes a new browser session.
OIDC linking, administrator recovery, managed service-account operations, and
audit CRUD remain reference-only endpoint inventory. CLI REST credentials omit
the `resource` parameter and use the Node issuer as `aud`; MCP credentials use
exactly `{issuer}/mcp` for both. DPoP `htu` is the scheme, authority, and path,
without query or fragment. The local CLI core workflow remains available
without a server.
- `POST /spaces/{space_id}/approvals`: a recently Passkey-authenticated human
  issues a one-time approval bound to `entry.delete`, `sql.delete`,
  `asset.delete`, or `access.put`. The issue response returns the one-time
  token; subsequent mutation requests send it only in
  `X-Ugoite-Human-Approval`. It is never echoed in a mutation JSON body,
  query string, log, or audit event.

Browser session cookies are opaque, server-side, and part of the supported v0.1
contract. TOTP remains a recovery-only factor and is not used for normal login.
Generic OAuth clients and remote asset upload remain future scope. Remote CLI
device credentials are supported through the browser-approved device flow. This
release does not provide a local authentication bypass.

## Authorization surfaces

Space CRUD, membership, Entries, Forms, Saved SQL, Assets, search, query, SQL
sessions, MCP, agents, and resource policies are represented in OpenAPI.
`PUT /spaces/{space_id}/policies/{kind}/{resource_id}` updates grant-only ACLs
for `entry` or `asset`.
CLI/agent delete and policy requests must include the approval header. The
server canonicalizes the route and strict mutation intent, hashes it with
SHA-256, atomically consumes the approval, and records the lifecycle in the
append-only Space audit chain. `403 HUMAN_APPROVAL_REQUIRED`,
`403 HUMAN_APPROVAL_INVALID`, `410 HUMAN_APPROVAL_EXPIRED`, and
`409 HUMAN_APPROVAL_REPLAYED` are stable failure codes.
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

## Response integrity headers

Eligible materialized responses from the shared `/api` route layer carry
`X-Ugoite-Key-Id` and `X-Ugoite-Signature`. The signature is lowercase
hexadecimal HMAC-SHA256 over the exact response body bytes delivered to the
caller. The key ID selects the operator-controlled HMAC material; verifiers
must not canonicalize or reserialize the body before checking it.

Responses for ordinary Node/API paths use the lazily created
`response_hmac/default.json` material under the configured Space operator root.
Requests under `/spaces/{space_id}/` use the
matching Space material. The server percent-decodes the Space path segment
exactly once and then applies the domain identifier rules. Unknown or invalid
Space identifiers do not create storage and remain unsigned.

Only bounded (at most 8 MiB), materialized, infallible JSON/string/bytes/empty
responses are eligible. SSE, streaming/fallible bodies, trailers, static
files, and explicit unsigned responses omit both headers; signing or storage
errors preserve the response without headers. There is no key-distribution
endpoint: operators provision or inspect keys through the storage boundary and
verify responses offline.

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
