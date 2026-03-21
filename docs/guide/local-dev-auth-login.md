# Local Development Authentication and Login

This is the canonical guide for the auth-aware `mise run dev` workflow.
`mise run dev` always routes auth setup through `scripts/dev-auth-env.sh`, and
README/docsite/developer-facing UI should link here instead of repeating
mode-specific auth steps inline.

The helper now prepares a **login context**, not an already-authenticated
session. The browser and CLI must sign in explicitly after startup, so local
development follows the same mental model as production: authenticate first,
receive a bearer token second.

The default path remains `manual-totp`, and you can opt into `mock-oauth`
when you want to exercise an explicit OAuth-style login flow. In every case,
the browser and CLI must sign in explicitly after startup.

## 1) Auth modes at a glance

| Mode | How to enable | What it does |
|---|---|---|
| `manual-totp` (default) | `mise run dev` | Prompts for a local admin username and validates a current 2FA code in the terminal, then exposes a browser/CLI login flow at `/login` or `ugoite auth login`. |
| `mock-oauth` | `UGOITE_DEV_AUTH_MODE=mock-oauth mise run dev` | Keeps startup unauthenticated, but exposes an explicit mock OAuth browser/CLI login path after the stack is running as the configured local admin user. |

Each backend/frontend dev process logs the active mode at startup, for example:

```text
Local dev auth mode: manual-totp (explicit username + 2FA login)
```

## 2) Install `oathtool` for manual TOTP flows

If you open the repository in the standard devcontainer, this step is already
handled for you during container setup.

If you are developing outside the devcontainer, install it manually:

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

The root task prepares the local auth context **once** before it fans out to the
backend, frontend, and docsite dev tasks. On a first `manual-totp` run, expect a
single username + 2FA prompt sequence before the backend and frontend start.

The helper waits for `http://localhost:8000/health` before the frontend dev
server starts, so `/api/*` requests do not race the backend startup.

On the first run (or after `UGOITE_DEV_AUTH_FORCE_LOGIN=true mise run dev`), the
terminal prompts for:

1. a local development admin username
2. a current 2FA code

The helper validates that code against `UGOITE_DEV_2FA_SECRET`, then stores the
resulting **login context** in `~/.ugoite/dev-auth.json` with owner-only
permissions (`0600`). The file contains the selected mode, username, and local
signing material for dev login sessions. It does **not** store an authenticated
bearer token and it does **not** start the app already logged in.

At backend startup, that configured user is also bootstrapped into the reserved
`admin-space`. Only active admins of `admin-space` can create additional spaces,
and each new space still makes its creator the initial owner/admin for that
space.

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

1. enter the same admin username you confirmed in the terminal
2. enter the current 2FA code from your authenticator (or from `oathtool`)
3. submit the form

Example TOTP generation:

```bash
oathtool --totp -b "${UGOITE_DEV_2FA_SECRET:-JBSWY3DPEHPK3PXP}"
```

The browser receives a signed bearer token only **after** the login form is
submitted successfully. The frontend stores that token in a local session cookie
for the `/api/*` proxy, so protected pages can render normally after login.
That login also grants the configured user access to the reserved
`admin-space`, which is what authorizes space creation in local development.

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
input, and the authenticated admin user can then create additional spaces.

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

## 9) Start one service at a time

Use the root `mise` tasks below when you want to keep the same auth-aware
startup flow but only run one service in the foreground:

```bash
mise run dev:backend
mise run dev:frontend
mise run dev:docsite
```

The backend and frontend tasks still source `scripts/dev-auth-env.sh`, and the
frontend task still waits for `http://localhost:8000/health` before it starts
proxying `/api/*` requests. The root `mise run dev` task simply prepares that
auth context once up front so the shared startup flow stays deterministic.
