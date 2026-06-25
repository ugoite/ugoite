# Error handling and resilience

## Server mapping

The Rust server maps typed core errors as follows:

| Core error kind | HTTP status |
|---|---:|
| invalid input | 422 |
| forbidden | 403 |
| not found | 404 |
| conflict | 409 |
| expired | 410 |
| unimplemented | 501 |
| dependency unavailable | 502 |
| internal | 500 |

Identifier parsing failures at the HTTP boundary use 400. Missing or invalid authentication uses 401. Explicitly unavailable login modes and rejected scope-enforced service identities use 403.

Typed core errors return a JSON object with stable `code` and safe `message` fields. Adapter-only errors may use a `detail` field. Unexpected failures must not expose filesystem paths, credentials, integrity secrets, or complete user content.

## Client behavior

- Treat 409 as a recoverable revision conflict and offer refresh/retry guidance.
- Treat 410 SQL sessions as expired and create a new session.
- Treat 501 as an intentionally unavailable capability rather than retrying indefinitely.
- Preserve the current editor value while displaying save failures.
- Do not interpret a 2xx response as durable success until the corresponding client decoder accepts the response body.

See [frontend–backend interface](../architecture/frontend-backend-interface.md) for transport ownership and protocol decoding.
