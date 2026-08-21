---
title: REST API
---

`crates/ugoite-server` and runtime `/openapi.json` are authoritative. The
checked-in OpenAPI snapshot and generated frontend path registry are verified in
CI.

The v0.1 server mutation boundary is local/memory-backed Space storage. S3-
compatible and other non-local operators remain available for supported
connection/configuration and derived CAS paths, but authoritative REST
mutations fail closed until a backend-level atomic multi-object fencing
contract is implemented. The stable error code is
`STORAGE_MUTATION_UNAVAILABLE`.

> Release boundary: this page documents the server endpoint inventory and
> implementation reference. Passkey/TOTP/OIDC login, managed service-account
> operations, audit CRUD, and remote CLI asset upload are not supported v0.1
> product capabilities. Treat those endpoint descriptions as future/reference
> material until the release boundary explicitly promotes them.

## Authentication surfaces

Interactive Passkey/TOTP/OIDC login, account recovery, OAuth device flow,
managed service-account operations, and audit CRUD are reference-only endpoint
inventory in this release. They are not shipped v0.1 capabilities and must not
be used as setup or login instructions. The implementation and checked-in
OpenAPI remain the source for future work; the current supported local workflow
is the operator-owned Space filesystem and its CLI commands.
- `POST /spaces/{space_id}/approvals`: a recently Passkey-authenticated human
  issues a one-time approval bound to `entry.delete`, `sql.delete`,
  `asset.delete`, or `access.put`. The issue response returns the one-time
  token; subsequent mutation requests send it only in
  `X-Ugoite-Human-Approval`. It is never echoed in a mutation JSON body,
  query string, log, or audit event.

The authentication headers, cookies, device credentials, and recovery material
described by the future endpoint inventory are not supported v0.1 behavior. This
release does not provide a local authentication bypass or a shipped credential
contract for those surfaces.

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
