# Authentication overview

The Rust server currently accepts:

1. development mock OAuth through `POST /auth/mock-oauth` when `UGOITE_DEV_AUTH_MODE=mock-oauth`, protected by `UGOITE_BOOTSTRAP_TOKEN`;
2. static bearer tokens from `UGOITE_AUTH_BEARER_TOKENS`;
3. static API keys from `UGOITE_AUTH_API_KEYS`;
4. signed bearer tokens validated with configured signing secrets and active key IDs.

`GET /auth/session` reports the current session and `DELETE /auth/session` clears it.

`POST /auth/login` is present in the contract but returns HTTP `403` in this release because passkey/TOTP enrollment and verification are not implemented by the Rust server. Do not present that path as operational.

Authentication establishes identity; Space membership and role checks authorize each operation separately.
