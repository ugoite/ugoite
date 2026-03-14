# Local Development Authentication and Login

This is the canonical guide for the auth-aware `mise run dev` workflow.
`mise run dev` always routes auth setup through `scripts/dev-auth-env.sh`, and
README/docsite/developer-facing UI should link here instead of repeating
mode-specific auth steps inline.

The default path remains automatic, but you can now opt into explicit manual
development auth modes when you want to debug login behavior instead of letting
the helper silently bootstrap everything for you.

## 1) Auth modes at a glance

| Mode | How to enable | What it does |
|---|---|---|
| `automatic` (default) | `mise run dev` | Uses `oathtool`, refreshes a cached local token, and exports matching backend/frontend bearer env vars automatically. |
| `manual-totp` | `UGOITE_DEV_AUTH_MODE=manual-totp ... mise run dev` | Skips automatic token generation and expects an explicit developer-provided TOTP step that is used locally to derive a deterministic bearer token for dev-only flows. |
| `mock-oauth` | `UGOITE_DEV_AUTH_MODE=mock-oauth ... mise run dev` | Uses a deterministic mock OAuth-style bearer token so you can intentionally exercise authenticated UI/API flows without the automatic TOTP bootstrap path. |

Each backend/frontend dev process logs the active mode at startup, for example:

```text
Local dev auth mode: manual-totp (token_source=UGOITE_DEV_TOTP_CODE)
```

## 2) Install oathtool for the automatic and manual TOTP flows

```bash
sudo apt-get update && sudo apt-get install -y oathtool
```

## 3) Development 2FA secret (dev-only plain secret)

For local development, the default Base32 TOTP secret is:

```text
JBSWY3DPEHPK3PXP
```

This is intentionally documented in plain text for development convenience only.
Do not reuse this secret in production.

Override it when needed:

```bash
export UGOITE_DEV_2FA_SECRET="YOUR_BASE32_SECRET"
```

## 4) Automatic mode (default)

```bash
mise run dev
```

The frontend dev server waits for `http://localhost:8000/health` before it starts,
so authenticated `/api/*` requests such as the spaces list do not hit the frontend
proxy before the backend is ready during local startup.

In `automatic` mode, the helper:

- generates a TOTP code using `oathtool`
- creates or refreshes a local bearer token
- stores it in `~/.ugoite/dev-auth.json` with local-user-only (`0600`) permissions
- exports matching `UGOITE_BOOTSTRAP_BEARER_TOKEN` and `UGOITE_AUTH_BEARER_TOKEN`

Force a fresh automatic login:

```bash
UGOITE_DEV_AUTH_FORCE_LOGIN=true mise run dev
```

Optional automatic-mode overrides:

```bash
export UGOITE_DEV_AUTH_FILE="$HOME/.ugoite/dev-auth.json"
export UGOITE_DEV_AUTH_TTL_SECONDS=43200
export UGOITE_DEV_USER_ID="dev-local-user"
```

## 5) Manual TOTP mode

Use `manual-totp` when you want to keep the dev secret and `oathtool` flow
visible and under your control instead of letting the helper refresh the cached
automatic token for you.

Preferred flow:

```bash
export UGOITE_DEV_AUTH_MODE=manual-totp
export UGOITE_DEV_TOTP_CODE="$(oathtool --totp -b "${UGOITE_DEV_2FA_SECRET:-JBSWY3DPEHPK3PXP}")"
mise run dev
```

Advanced override if you want to pick the bearer token yourself:

```bash
export UGOITE_DEV_AUTH_MODE=manual-totp
export UGOITE_DEV_MANUAL_TOKEN="dev-manual-auth-token"
mise run dev
```

`manual-totp` also respects pre-exported matching auth variables:

```bash
export UGOITE_DEV_AUTH_MODE=manual-totp
export UGOITE_AUTH_BEARER_TOKEN="dev-manual-auth-token"
export UGOITE_BOOTSTRAP_BEARER_TOKEN="dev-manual-auth-token"
mise run dev
```

Notes:

- The current local dev stack does not call a separate auth server. Manual mode
  still works by exporting the bearer token into both the backend bootstrap path
  and the frontend proxy path.
- `UGOITE_DEV_TOTP_CODE` is the explicit manual step that keeps the CLI
  `oathtool` invocation visible when you want to debug or demonstrate the local
  2FA workflow.
- The current `manual-totp` implementation does not validate that code against a
  separate auth service or re-check it against `UGOITE_DEV_2FA_SECRET`. It uses
  the provided value locally to derive a deterministic bearer token for the dev
  backend/frontend pair.

## 6) Mock OAuth mode

Use `mock-oauth` when you want an explicit, reproducible login-like path without
relying on the automatic `oathtool` bootstrap.

```bash
export UGOITE_DEV_AUTH_MODE=mock-oauth
mise run dev
```

Optional overrides:

```bash
export UGOITE_DEV_AUTH_MODE=mock-oauth
export UGOITE_DEV_USER_ID="dev-oauth-user"
export UGOITE_DEV_MOCK_OAUTH_TOKEN="mock-oauth-dev-token"
mise run dev
```

The default mock token is deterministic for the chosen `UGOITE_DEV_USER_ID`, so
the backend and frontend dev tasks keep sharing the same token during one local
session without needing a cached auth file.

## 7) Verify auth locally

1. Open `http://localhost:3000`.
2. Confirm API calls include `Authorization: Bearer <token>`.
3. Check backend health (`/health` is intentionally unauthenticated):

```bash
curl -i http://localhost:8000/health
```

Expected response: `HTTP/1.1 200 OK` with body `{"status":"ok"}`.

## 8) Start one service at a time

Use the root `mise` tasks below when you want to keep the same auth-aware
startup flow but only run one service in the foreground:

```bash
mise run dev:backend
mise run dev:frontend
mise run dev:docsite
```

The backend and frontend tasks still source `scripts/dev-auth-env.sh`, and the
frontend task still waits for `http://localhost:8000/health` before it starts
proxying `/api/*` requests.
