# Environment Variable Matrix

This matrix summarizes primary runtime variables by mode.

| Variable | Local dev | E2E runner | Docker Compose | CI |
|---|---|---|---|---|
| BACKEND_URL | required for frontend proxy | set by runner | set to backend service URL | required in frontend jobs |
| UGOITE_FRONTEND_BEARER_TOKEN | optional | required | optional | optional |
| UGOITE_ROOT | optional | required | volume-backed path | test workspace path |
| UGOITE_ALLOW_REMOTE | optional | required | required | required for API tests |
| UGOITE_AUTH_BEARER_TOKENS_JSON | optional | required | optional | optional |
| E2E_AUTH_BEARER_TOKEN | n/a | required | n/a | required for e2e jobs |
| E2E_BACKEND_PORT | n/a | optional | n/a | optional |
| E2E_FRONTEND_PORT | n/a | optional | n/a | optional |

Use mode-specific `.env` files to avoid mixing values between workflows.
