# Environment Variable Matrix

This matrix summarizes primary runtime variables by mode. `E2E runner` refers to
the direct-process helper in `e2e/scripts/run-e2e.sh`, while `Docker Compose`
refers to the CI-parity compose helper in `e2e/scripts/run-e2e-compose.sh`.

| Variable | Local dev | E2E runner | Docker Compose | CI |
|---|---|---|---|---|
| BACKEND_URL | required for frontend proxy | set by runner | set to backend service URL | required in frontend jobs |
| FRONTEND_URL | optional explicit frontend origin for SSR `/api` fetches when framework origin envs are unavailable | set by runner | optional | optional |
| UGOITE_ROOT | optional | required | volume-backed path | test workspace path |
| UGOITE_AUTH_API_KEY | optional | optional | optional | optional |
| UGOITE_AUTH_BEARER_TOKENS | optional | required for static secondary e2e users | required for static secondary e2e users | optional |
| UGOITE_AUTH_BEARER_SIGNING_SECRETS | optional | required | required for signed bearer verification | optional |
| UGOITE_AUTH_BEARER_ACTIVE_KIDS | optional, emitted by `dev-auth-env.sh` | required | auto-derived from `UGOITE_DEV_SIGNING_KID` when omitted | optional |
| UGOITE_PROXY_TIMEOUT_MS | optional | optional | optional | optional |
| UGOITE_DEV_AUTH_FILE | optional | n/a | n/a | n/a |
| UGOITE_DEV_AUTH_TTL_SECONDS | optional | optional | optional | optional |
| UGOITE_DEV_AUTH_FORCE_LOGIN | optional | n/a | n/a | n/a |
| UGOITE_DEV_AUTH_MODE | optional, defaults to `mock-oauth` in dev tooling | required (`mock-oauth`) | required (`mock-oauth`) | optional |
| UGOITE_DEV_PASSKEY_CONTEXT | planned passkey/TOTP support only | n/a | n/a | n/a |
| UGOITE_DEV_USER_ID | optional | required | required | optional |
| UGOITE_DEV_SIGNING_SECRET | optional, emitted by `dev-auth-env.sh` when absent | required | optional unless signed bearer login is enabled | optional |
| UGOITE_DEV_SIGNING_KID | optional, emitted by `dev-auth-env.sh` | required | optional unless signed bearer login is enabled | optional |
| E2E_AUTH_BEARER_TOKEN | n/a | generated after mock-oauth login | generated after mock-oauth login | required for e2e jobs |
| E2E_STORAGE_ROOT | n/a | required | n/a | optional |
| E2E_BACKEND_PORT | n/a | optional | n/a | optional |
| E2E_FRONTEND_PORT | n/a | optional | n/a | optional |
| E2E_FRONTEND_MODE | n/a | optional | n/a | optional |
| E2E_ENFORCE_CI_GATES | n/a | optional | n/a | n/a |
| E2E_BUILD_IMAGES | n/a | n/a | optional (`true` for local parity runs) | set to `false` because CI pre-builds the images |
| E2E_BACKEND_START_TIMEOUT_SECONDS | n/a | n/a | optional | optional |
| E2E_FRONTEND_START_TIMEOUT_SECONDS | n/a | n/a | optional | optional |
| E2E_TEST_TIMEOUT_MS | n/a | optional | optional | optional |

Use mode-specific `.env` files to avoid mixing values between workflows.
