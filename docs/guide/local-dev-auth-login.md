# Local Development Authentication and Login

`mise run dev` always routes auth setup through `scripts/dev-auth-env.sh`.
The helper now prepares a **login context**, not an already-authenticated
session. The browser and CLI must sign in explicitly after startup, so local
development follows the same mental model as production: authenticate first,
receive a bearer token second.

## 1) Auth modes at a glance

| Mode | How to enable | What it does |
|---|---|---|
| `manual-totp` (default) | `mise run dev` | Prompts for a local username and validates a current 2FA code in the terminal, then exposes a browser/CLI login flow at `/login` or `ugoite auth login`. |
| `mock-oauth` | `UGOITE_DEV_AUTH_MODE=mock-oauth mise run dev` | Keeps startup unauthenticated, but exposes an explicit mock OAuth browser/CLI login path after the stack is running. |

Each backend/frontend dev process logs the active mode at startup, for example:

```text
Local dev auth mode: manual-totp (login_user=dev-local-user auth_context=updated)
```

## 2) Install `oathtool` for manual TOTP flows

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

## 4) Start the dev stack

```bash
mise run dev
```

The helper waits for `http://localhost:8000/health` before the frontend dev
server starts, so `/api/*` requests do not race the backend startup.

On the first run (or after `UGOITE_DEV_AUTH_FORCE_LOGIN=true mise run dev`), the
terminal prompts for:

1. a local development username
2. a current 2FA code

The helper validates that code against `UGOITE_DEV_2FA_SECRET`, then stores the
resulting **login context** in `~/.ugoite/dev-auth.json` with owner-only
permissions (`0600`). The file contains the selected mode, username, and local
signing material for dev login sessions. It does **not** store an authenticated
bearer token and it does **not** start the app already logged in.

Force a fresh prompt:

```bash
UGOITE_DEV_AUTH_FORCE_LOGIN=true mise run dev
```

Optional startup overrides:

```bash
export UGOITE_DEV_USER_ID="dev-local-user"
export UGOITE_DEV_AUTH_TTL_SECONDS=43200
```

## 5) Browser login (`manual-totp`)

After the stack is running, open:

```text
http://localhost:3000/login
```

Then:

1. enter the same username you confirmed in the terminal
2. enter the current 2FA code from your authenticator (or from `oathtool`)
3. submit the form

Example TOTP generation:

```bash
oathtool --totp -b "${UGOITE_DEV_2FA_SECRET:-JBSWY3DPEHPK3PXP}"
```

The browser receives a signed bearer token only **after** the login form is
submitted successfully. The frontend stores that token in a local session cookie
for the `/api/*` proxy, so protected pages can render normally after login.

## 6) CLI login (`manual-totp`)

Configure the CLI to target the backend directly:

```bash
cargo run -q -p ugoite-cli -- config set --mode backend --backend-url http://localhost:8000
```

Then log in explicitly:

```bash
cargo run -q -p ugoite-cli -- auth login --username dev-local-user --totp-code 123456
```

The command prints an `export UGOITE_AUTH_BEARER_TOKEN=...` line for the current
shell. That token is minted only after the backend validates the username + 2FA
input.

## 7) Mock OAuth mode

Use `mock-oauth` when you want an explicit OAuth-style login path without
pre-authenticating the stack at startup.

```bash
export UGOITE_DEV_AUTH_MODE=mock-oauth
mise run dev
```

Browser flow:

1. open `http://localhost:3000/login`
2. click **Continue with Mock OAuth**

CLI flow:

```bash
cargo run -q -p ugoite-cli -- config set --mode backend --backend-url http://localhost:8000
cargo run -q -p ugoite-cli -- auth login --mock-oauth
```

## 8) Verify auth locally

1. Open `http://localhost:3000/login`.
2. Complete either the username + 2FA form or the mock OAuth button.
3. Confirm protected pages such as `/spaces` load successfully.
4. Check backend health (`/health` is intentionally unauthenticated):

```bash
curl -i http://localhost:8000/health
```

Expected response: `HTTP/1.1 200 OK` with body `{"status":"ok"}`.
