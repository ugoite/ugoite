# Local Development Authentication and Login

This is the canonical guide for the auth-aware `mise run dev` workflow and the
local browser login flow.

The current Rust server ships one implemented local interactive login mode:
`mock-oauth`. It is still an explicit login flow, not a pre-authenticated
startup shortcut. The browser and CLI receive a bearer token only after they
call the login endpoint.

`passkey-totp` remains planned for a future release. The server does not
advertise it as supported, and the login UI should not show it as an available
method until the Rust implementation is complete end to end.

## Auth Mode Reference

| Mode | How to enable | Current status |
| --- | --- | --- |
| `mock-oauth` | Default for `mise run dev`, Docker Compose, and E2E runners | Implemented local demo login with no external OAuth provider |
| `passkey-totp` | Do not use as a release gate yet | Planned; not advertised by `/auth/config` in the current Rust server |

## Start The Dev Stack

```bash
mise run dev
```

Unless you override them, the dev task starts the Rust backend with:

```text
UGOITE_DEV_AUTH_MODE=mock-oauth
UGOITE_DEV_USER_ID=dev-local-user
UGOITE_BOOTSTRAP_TOKEN=dev-token
UGOITE_BOOTSTRAP_DEFAULT_SPACE=true
```

The backend bootstraps the configured user as an active admin member of the
starter spaces it creates. Each new space created over HTTP also records its
creator as the initial active admin member.

## Browser Login

Open:

```text
http://localhost:3000/login
```

Then click **Continue with Local Demo Login**. The frontend stores the returned
bearer token in an HttpOnly local session cookie for proxied `/api/*` requests,
so protected pages such as `/spaces` render only after the login step succeeds.

Once `/spaces` loads, continue to
[Browser Walkthrough: First Space, Form, and Entry](browser-first-entry.md).

## CLI Login

Configure the CLI to target the backend:

```bash
cargo run -q -p ugoite-cli -- config set --mode backend --backend-url http://localhost:8000
cargo run -q -p ugoite-cli -- auth login --mock-oauth
```

If you installed the published CLI, use:

```bash
ugoite config set --mode backend --backend-url http://localhost:8000
ugoite auth login --mock-oauth
```

The command saves a CLI session for follow-up commands and also prints
shell-ready environment commands. Use `--shell fish` or `--shell powershell`
when you want shell-native output.

## Verify Auth Locally

1. Open `http://localhost:3000/login`.
2. Complete **Continue with Local Demo Login**.
3. Confirm `/spaces` loads.
4. Check backend health:

```bash
curl -i http://localhost:8000/health
```

Expected response: `HTTP/1.1 200 OK` with body `{"status":"ok"}`.
