---
title: 'Environment variable matrix'
---

| Variable | Purpose | Default |
|---|---|---|
| `UGOITE_ROOT` | authoritative workspace root | `./data`; image sets `/data` |
| `UGOITE_SERVER_ADDRESS` | HTTP listen address | `127.0.0.1:8000`; image sets `0.0.0.0:8000` |
| `UGOITE_STATIC_DIR` | compiled browser files | unset; image sets `/app/static` |
| `UGOITE_BOOTSTRAP_DEFAULT_SPACE` | create initial default Space when true | false unless set |
| `UGOITE_DEV_AUTH_MODE` | development auth mode | `mock-oauth` |
| `UGOITE_DEV_USER_ID` | development/bootstrap identity | `dev-local-user` |
| `UGOITE_BOOTSTRAP_TOKEN` | secret required by mock OAuth | unset |
| `UGOITE_AUTH_BEARER_TOKENS` | static bearer credential set | unset |
| `UGOITE_AUTH_API_KEYS` | static API-key credential set | unset |
| `UGOITE_AUTH_BEARER_SIGNING_SECRETS` | `kid:secret` signing keys | unset |
| `UGOITE_AUTH_BEARER_ACTIVE_KIDS` | active signing key IDs | unset |
| `UGOITE_AUTH_REVOKED_KEY_IDS` | revoked signing key IDs | unset |
| `UGOITE_VERSION` | release image tag | required by release Compose |
| `UGOITE_PORT` | release host port | `8000` |
| `UGOITE_SPACES_DIR` | release host storage directory | `./spaces` |

Historical split-stack and alternate package-manager variables are not part of the current runtime.
