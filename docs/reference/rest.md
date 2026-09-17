---
title: "REST API and OpenAPI"
description: Auth model, error model, and where endpoint truth lives.
sidebar:
  order: 3
---

The server implementation in `crates/ugoite-server` and the generated contract
at `/openapi.json` are authoritative. The checked-in snapshot at
`crates/ugoite-server/src/openapi.json` is generated and drift-checked; do not
copy endpoints or schemas into prose and do not maintain a second protocol
registry here.

## Start here

Read the [REST overview](../architecture/api/rest.md) for admission,
storage-boundary, and authentication surfaces. Use `/openapi.json` from a
running server for exact paths, methods, and schemas. Examples of contract
paths (not a registry: verify against the generated document) include
`/health`, `/spaces`, `/spaces/{space_id}/entries`,
`/spaces/{space_id}/entries/{entry_id}/history`, and
`/spaces/{space_id}/search`.

## Auth model

Passkey and WebAuthn browser sessions, owner-approved recovery, Remote CLI
device credentials with DPoP, recovery-only TOTP Account Self-Recovery, Space
membership and ACL enforcement, authenticated MCP access, and invitation-gated
OIDC. Administrator recovery, agent principals, generic OAuth compatibility, and
audit CRUD remain future or reference-only.

## Error model

Failures use stable codes such as `STORAGE_MUTATION_UNAVAILABLE` for unverified
backends. Validation failures return safe detail without exposing storage
layout. See [Error handling](../architecture/quality/error-handling.md).

Structured Entry writes reject a key supplied in both `fields` and
`extra_attributes` (or an extra shadowing a real field name) with exactly
`{"code": "INVALID_INPUT", "detail": {"duplicate_fields": ["FieldA",
"FieldB"]}}`: `duplicate_fields` is a lexically sorted string array and
message text is not contract.

## Related

- `cargo run -p xtask -- openapi-check` verifies the contract.
