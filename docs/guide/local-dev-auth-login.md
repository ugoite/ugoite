# Local Development Authentication and Login

This is the canonical guide for the auth-aware `mise run dev` workflow.
`mise run dev` always routes auth setup through `scripts/dev-auth-env.sh`, and
README/docsite/developer-facing UI should link here instead of repeating
mode-specific auth steps inline.

The helper now prepares a **login context**, not an already-authenticated
session. The browser and CLI must sign in explicitly after startup, so local
development follows the same mental model as production: authenticate first,
receive a bearer token second.

The default path remains `passkey-totp`, and you can opt into the local demo
login path (`mock-oauth`) when you want an explicit browser/CLI sign-in flow
without an external provider. In every case, the browser and CLI must sign in
explicitly after startup.

That default intentionally differs from the published
[`docker-compose.release.yaml` quick start](container-quickstart.md), which
uses the local demo login mode (`mock-oauth`) so newcomers can evaluate the
browser flow faster. Source development keeps `passkey-totp` on by default so
contributors exercise the explicit passkey + 2FA login path that
`mise run dev` wires through `scripts/dev-auth-env.sh`. If you want source
development to mirror the release quick start instead, set
`UGOITE_DEV_AUTH_MODE=mock-oauth` before startup.

## 1) Auth modes at a glance

| Mode | How to enable | What it does |
|---|---|---|
| `passkey-totp` (default) | `mise run dev` | Prompts for a local admin username and validates a current 2FA code in the terminal, then creates a passkey-bound local context that browser/CLI login requests must present alongside the 2FA step. |
| `mock-oauth` | `UGOITE_DEV_AUTH_MODE=mock-oauth mise run dev` | Keeps startup unauthenticated, but exposes an explicit local demo browser/CLI login path after the stack is running as the configured local admin user. No external OAuth provider is used. |

Each backend/frontend dev process logs the active mode at startup, for example:

```text
Local dev auth mode: passkey-totp (passkey-bound username + 2FA login)
```

## 2) Install `oathtool` for passkey + TOTP flows

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
backend, frontend, and docsite dev tasks. On a first `passkey-totp` run, expect a
single username + 2FA prompt sequence before the backend and frontend start.

The helper waits for `http://localhost:8000/health` before the frontend dev
server starts, so `/api/*` requests do not race the backend startup.

Unless you already exported `UGOITE_ROOT`, the helper also exports
`UGOITE_ROOT=<repo root>` so the dev backend reads the same `./spaces` tree
that `mise run seed` targets by default.

On the first run (or after `UGOITE_DEV_AUTH_FORCE_LOGIN=true mise run dev`), the
terminal prompts for:

1. a local development admin username
2. a current 2FA code

The helper validates that code against `UGOITE_DEV_2FA_SECRET`, then stores the
resulting **login context** in `~/.ugoite/dev-auth.json` with owner-only
permissions (`0600`). The file contains the selected mode, username, signing
material, and a reusable `UGOITE_DEV_PASSKEY_CONTEXT` value. The frontend proxy
and CLI forward that passkey-bound local context automatically during
`passkey-totp` login requests. The file does **not** store an authenticated
bearer token and it does **not** start the app already logged in.

At backend startup, that configured user is also bootstrapped into the reserved
`admin-space`. Only active admins of `admin-space` can create additional spaces,
and each new space still makes its creator the initial admin for that
space.

Force a fresh prompt:

```bash
UGOITE_DEV_AUTH_FORCE_LOGIN=true mise run dev
```

Optional startup overrides:

```bash
export UGOITE_DEV_USER_ID="dev-local-user"
export UGOITE_DEV_AUTH_TTL_SECONDS=43200
export UGOITE_DEV_PASSKEY_CONTEXT="optional-local-passkey-context"
```

## 5) Browser login (`passkey-totp`)

After the stack is running, open:

```text
http://localhost:3000/login
```

Then:

1. enter the same admin username you confirmed in the terminal
2. enter the current 2FA code from your authenticator (or from `oathtool`)
3. submit the form from the same local dev session that prepared the passkey context

Example TOTP generation:

```bash
oathtool --totp -b "${UGOITE_DEV_2FA_SECRET:-JBSWY3DPEHPK3PXP}"
```

The browser receives a signed bearer token only **after** the login form is
submitted successfully. The frontend proxy attaches the local
`UGOITE_DEV_PASSKEY_CONTEXT` automatically, then stores the resulting bearer
token in an HttpOnly local session cookie for `/api/*` so frontend JavaScript
never reads the raw token and protected pages can render normally after login.
Repeated invalid passkey + 2FA submissions temporarily return `429 Too Many Requests`
with a `Retry-After` header so the local login surface cannot be hammered indefinitely.
That login also grants the configured user access to the reserved
`admin-space`, which is what authorizes space creation in local development.

## 6) CLI login (`passkey-totp`)

Configure the CLI to target the backend directly:

```bash
cargo run -q -p ugoite-cli -- config set --mode backend --backend-url http://localhost:8000
```

Then log in explicitly:

```bash
cargo run -q -p ugoite-cli -- auth login --username dev-local-user --totp-code 123456
```

If you installed the published CLI, run the equivalent `ugoite auth login`
command with the same flags.

The command saves a CLI session so later `ugoite` commands stay authenticated
without extra shell setup, and it also prints an
`export UGOITE_AUTH_BEARER_TOKEN=...` line for the current shell. That token is
minted only after the backend validates the username + 2FA input together with
the local `UGOITE_DEV_PASSKEY_CONTEXT`, and the authenticated admin user can
then create additional spaces.
Repeated invalid login attempts hit the same temporary `429 Too Many Requests`
throttle the browser flow uses.

## 7) Local demo login (`mock-oauth`) mode

Use `mock-oauth` when you want the explicit local demo login path without
pre-authenticating the stack at startup.

```bash
export UGOITE_DEV_AUTH_MODE=mock-oauth
mise run dev
```

Browser flow:

1. open `http://localhost:3000/login`
2. click **Continue with Local Demo Login**

CLI flow:

```bash
cargo run -q -p ugoite-cli -- config set --mode backend --backend-url http://localhost:8000
cargo run -q -p ugoite-cli -- auth login --mock-oauth
```

## 8) Verify auth locally

1. Open `http://localhost:3000/login`.
2. Complete either the passkey-bound username + 2FA form or the local demo login button.
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
