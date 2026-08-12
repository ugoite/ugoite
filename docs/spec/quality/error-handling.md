---
title: "Error handling and resilience"
---

This specification defines stable server error mappings, safe response content,
and the recovery behavior expected from clients.

## Server mapping

The Rust server maps typed core errors as follows:

| Core error kind        | HTTP status |
| ---------------------- | ----------: |
| invalid input          |         422 |
| forbidden              |         403 |
| not found              |         404 |
| conflict               |         409 |
| expired                |         410 |
| unimplemented          |         501 |
| dependency unavailable |         502 |
| internal               |         500 |

Identifier parsing failures at the HTTP boundary use 400. Missing or invalid
authentication uses 401. Explicitly unavailable login modes and rejected
scope-enforced service identities use 403.

Typed core errors return a JSON object with stable `code` and safe `message`
fields. Adapter-only errors may use a `detail` field. Unexpected failures must
not expose filesystem paths, credentials, integrity secrets, or complete user
content.

## Client behavior

- Treat 409 as a recoverable revision conflict and offer refresh/retry guidance.
- Treat 410 SQL sessions as expired and create a new session.
- Treat 501 as an intentionally unavailable capability rather than retrying
  indefinitely.
- Preserve the current editor value while displaying save failures.
- Do not interpret a 2xx response as durable success until the corresponding
  client decoder accepts the response body.
- Recovery one-time responses use `Cache-Control: no-store`. Treat
  `OWNER_RESET_ALREADY_COMPLETED` and `BACKUP_ROTATION_ALREADY_COMMITTED` as
  terminal; do not replay the token or a committed backup-rotation key.
  `audit_status: pending`
  means the credential/code mutation committed and audit delivery is queued.
  `RECOVERY_FENCE_UNAVAILABLE` is a 409: a committed recovery marker still
  holds an unreconciled Space fence and must be resolved by the restart-safe
  reconciler. `RECOVERY_STORAGE_UNAVAILABLE` is a 503 only for failures before
  the Node CAS commits any credential or code mutation.

See [frontend–backend interface](../architecture/frontend-backend-interface.md)
for transport ownership and protocol decoding.
