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
- After a lost mutation response, reconcile the immutable target before
  replaying intent. Entry creates use their explicit ID as the reconciliation
  key; Entry updates use `parent_revision_id` and a concurrent change remains a
  409 conflict. Automatic blind retries and last-write-wins are not supported.
- A server restart preserves browser sessions only when the configured Node
  control store and node secret are preserved. A 401 means authentication is
  absent or invalid; a 403/404 for a Space is an authorization or Space-state
  result and must be shown separately.
- Recovery one-time responses use `Cache-Control: no-store`. Treat
  `SPACE_RECOVERY_ALREADY_COMPLETED` as terminal; do not replay the token.
  `audit_status: pending` means the Space binding mutation committed and audit
  delivery is queued.
  `RECOVERY_FENCE_UNAVAILABLE` is a 409: a committed recovery marker still
  holds an unreconciled Space fence and must be resolved by the restart-safe
  reconciler. `RECOVERY_STORAGE_UNAVAILABLE` is a 503 only for failures before
  the Node CAS commits any credential or code mutation.

See [frontend–backend interface](../../architecture/contracts/frontend-backend-interface.md)
for transport ownership and protocol decoding.
