# Environment Variable Matrix

This matrix summarizes primary runtime variables by mode.

| Variable | Local dev | E2E runner | Docker Compose | CI |
|---|---|---|---|---|
| BACKEND_URL | required for frontend proxy | set by runner | set to backend service URL | required in frontend jobs |
| UGOITE_AUTH_BEARER_TOKEN | auto-managed by `dev-auth-env.sh` | required | optional | optional |
| UGOITE_ROOT | optional | required | volume-backed path | test workspace path |
| UGOITE_ALLOW_REMOTE | optional | required | required | required for API tests |
| UGOITE_AUTH_BEARER_TOKENS_JSON | optional | required | optional | optional |
| UGOITE_AUTH_API_KEY | optional | optional | optional | optional |
| UGOITE_BOOTSTRAP_BEARER_TOKEN | auto-managed by `dev-auth-env.sh` | optional | optional | optional |
| UGOITE_BOOTSTRAP_USER_ID | auto-managed by `dev-auth-env.sh` | optional | optional | optional |
| UGOITE_PROXY_TIMEOUT_MS | optional | optional | optional | optional |
| UGOITE_DEV_AUTH_FILE | optional | n/a | n/a | n/a |
| UGOITE_DEV_AUTH_TTL_SECONDS | optional | n/a | n/a | n/a |
| UGOITE_DEV_AUTH_FORCE_LOGIN | optional | n/a | n/a | n/a |
| UGOITE_DEV_2FA_SECRET | optional | n/a | n/a | n/a |
| E2E_AUTH_BEARER_TOKEN | n/a | required | n/a | required for e2e jobs |
| E2E_STORAGE_ROOT | n/a | required | n/a | optional |
| E2E_BACKEND_PORT | n/a | optional | n/a | optional |
| E2E_FRONTEND_PORT | n/a | optional | n/a | optional |
| E2E_FRONTEND_MODE | n/a | optional | n/a | optional |
| E2E_TEST_TIMEOUT_MS | n/a | optional | n/a | optional |

Use mode-specific `.env` files to avoid mixing values between workflows.
