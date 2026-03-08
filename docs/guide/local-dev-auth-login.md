# Local Development Authentication and Login

`mise run dev` now runs an automatic local login flow and token persistence step
via `scripts/dev-auth-env.sh`. You do not need to export bearer env vars
manually for the common dev path.

## 1) Install oathtool (required for local 2FA code generation)

```bash
sudo apt-get update && sudo apt-get install -y oathtool
```

## 2) Development 2FA secret (dev-only plain secret)

For local development, the script uses this default Base32 secret:

```text
JBSWY3DPEHPK3PXP
```

This is intentionally documented in plain text for development convenience only.
Do not reuse this secret in production.

You can override it with:

```bash
export UGOITE_DEV_2FA_SECRET="YOUR_BASE32_SECRET"
```

## 3) Run development

```bash
mise run dev
```

The frontend dev server waits for `http://localhost:8000/health` before it starts,
so authenticated `/api/*` requests such as the spaces list do not hit the frontend
proxy before the backend is ready during local startup.

On first run (or after expiry), the script:
- generates a TOTP code using `oathtool`
- creates a local bearer token
- stores it in `~/.ugoite/dev-auth.json`
- exports backend/frontend auth env vars automatically

## 4) Token reuse and re-login

- Cached token is reused while valid (default TTL: 12 hours).
- Expired token triggers automatic re-login flow.
- Force re-login manually:

```bash
UGOITE_DEV_AUTH_FORCE_LOGIN=true mise run dev
```

Optional overrides:

```bash
export UGOITE_DEV_AUTH_FILE="$HOME/.ugoite/dev-auth.json"
export UGOITE_DEV_AUTH_TTL_SECONDS=43200
export UGOITE_DEV_USER_ID="dev-local-user"
```

## 5) Verify auth locally

1. Open `http://localhost:3000`.
2. Confirm API calls include `Authorization: Bearer <token>`.
3. Check backend health (`/health` is intentionally unauthenticated):

```bash
curl -i http://localhost:8000/health
```

Expected response: `HTTP/1.1 200 OK` with body `{"status":"ok"}`.
