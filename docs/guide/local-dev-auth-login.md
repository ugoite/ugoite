# Local Development Authentication and Login

When auth is enabled, opening `http://localhost:3000` without a valid bearer token can return `Unauthorized`.
Use one of the following local setups before running `mise run dev`.

## Option 1: Single bootstrap token

Set one token that the backend accepts and the frontend sends.

```bash
export UGOITE_BOOTSTRAP_BEARER_TOKEN="dev-local-token"
export UGOITE_AUTH_BEARER_TOKEN="dev-local-token"
mise run dev
```

## Option 2: Explicit token list

Use JSON to define accepted tokens and keep the frontend token in sync.

```bash
export UGOITE_AUTH_BEARER_TOKENS_JSON='["dev-local-token","another-token"]'
export UGOITE_AUTH_BEARER_TOKEN="dev-local-token"
mise run dev
```

## Verify login/auth locally

1. Open `http://localhost:3000`.
2. Open browser devtools and confirm API calls include `Authorization: Bearer <token>`.
3. Check backend health (the `/health` endpoint is intentionally unauthenticated and should return `200 OK`):

```bash
curl -i http://localhost:8000/health
```

Expected response: `HTTP/1.1 200 OK` with body `{"status":"ok"}`.

If the page still shows `Unauthorized`, follow [Troubleshooting Unauthorized Spaces](./troubleshooting-unauthorized-spaces.md).
