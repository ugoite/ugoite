# Authentication Overview

Use this guide when you want the human-facing explanation of how authentication
works across the browser, CLI, and backend today.

This guide explains the current auth model. If you want the practical
step-by-step setup flow, continue to
[Local Development Authentication and Login](local-dev-auth-login.md) next,
whether you have already started the stack or are still deciding how to do it.
That guide is the hands-on workflow for `mise run dev`, `/login`, and
`ugoite auth login`.

If you need the machine-readable snapshot of the current auth contract, run:

```bash
ugoite auth overview
```

That command prints the runtime export from `ugoite-core`. It is useful for
tooling and contract checks, but it is not the easiest place for a newcomer to
learn the model.

## What exists today

Today, Ugoite enforces authentication for browser, CLI-via-backend, REST, and
MCP access. The implemented authentication building blocks are:

- signed or static **bearer tokens** for interactive user sessions
- **API keys** for service-style access
- explicit local development login flows for `passkey-totp` and the local demo
  login mode (`mock-oauth`)

Some security specifications also describe future passkey/WebAuthn directions.
Treat those as planned work unless a guide explicitly tells you they are already
implemented.

## Which surface uses which auth path?

| Surface | What you normally do | Token or credential shape |
| --- | --- | --- |
| Browser frontend | Sign in on `/login` after the backend advertises the active dev auth mode | HttpOnly browser session cookie used by the frontend proxy for `/api/*` requests |
| CLI in `backend` / `api` mode | Run `ugoite auth login` or provide an API key / bearer token env var | Bearer token or API key |
| Backend REST / MCP clients | Send `Authorization: Bearer ...` or configured API keys | Bearer token or API key |
| CLI in `core` mode | No backend auth flow because the CLI talks to local storage directly | No backend credential required |

The important mental model is that Ugoite separates **where you run** from
**how you authenticate**:

- `core` mode is local-first direct filesystem access from the CLI
- `backend` / `api` modes send requests to a server and therefore need
  authentication

For CLI troubleshooting, pair `ugoite config current` with `ugoite auth profile`:
the first command tells you which topology is active, and the second tells you
whether the current mode needs backend credentials or already has one available.

## Local development modes at a glance

When you run `mise run dev`, the backend exposes one of two explicit login
experiences:

| Mode | What it is for | How login happens |
| --- | --- | --- |
| `passkey-totp` | Default local development path | You choose a local admin username, prove a current 2FA code, then sign in explicitly in the browser or CLI |
| `mock-oauth` | Development-only local demo login path | You still sign in explicitly after startup, but the backend issues a bearer token through the local demo login route instead of username + TOTP. No external OAuth provider is involved. |

Both modes are intentionally **explicit login** flows. Startup prepares login
context; it does not silently inject an already-authenticated session.

## Browser login in plain language

The browser experience is meant to feel like a real application session:

1. open `/login`
2. discover which local dev auth mode is active
3. complete that login flow
4. receive a bearer token only after successful authentication

In `passkey-totp`, the form asks for the same username and current 2FA code that
match your local development setup.

In `mock-oauth`, the page offers an explicit local demo login action instead.

After login, the frontend proxy stores the bearer token in an HttpOnly browser
session cookie for proxied `/api/*` requests. Frontend JavaScript does not read
the raw token directly. That is why protected pages such as `/spaces` work only
after the login step succeeds.

Once `/spaces` loads, continue to
[Browser Walkthrough: First Space, Form, and Entry](browser-first-entry.md) for
the exact newcomer path from login to the first useful space content.

## CLI login in plain language

The CLI only uses the backend login flow when it is pointed at a server:

```bash
ugoite config set --mode backend --backend-url http://localhost:8000
ugoite auth login
```

You can also provide the values directly:

```bash
ugoite auth login --username dev-local-user --totp-code 123456
ugoite auth login --mock-oauth
```

The command saves a CLI session under the ugoite config home so later `ugoite`
commands stay authenticated without `eval`, and it also prints shell-ready
environment commands for the current shell session. By default it emits POSIX
`export` / `unset` syntax; pass `--shell fish` or `--shell powershell` when you
want `source` / `Invoke-Expression` friendly output instead. The same flag also
works with `ugoite auth token-clear` and `ugoite auth logout`.

If the CLI is still in `core` mode, `ugoite auth login` correctly refuses to
run because there is no backend auth exchange in that topology.

## Bearer tokens vs API keys

For most newcomer workflows, think of the credentials like this:

- **Bearer token**: interactive session credential for a user signing in through
  the browser or CLI
- **API key**: service-style credential for automation or non-interactive access

The runtime auth overview also exposes broader capability details such as static
token support, signed token support, active signing key IDs, and service-account
key support. Those details matter for operators and tooling, but a newcomer can
usually start with "users sign in for bearer tokens; automation can use API
keys."

## Where to go next

- Need the concrete post-login browser path? Read
  [Browser Walkthrough: First Space, Form, and Entry](browser-first-entry.md).
- Need the exact local login steps? Read
  [Local Development Authentication and Login](local-dev-auth-login.md).
- Need CLI usage details? Read [CLI Guide](cli.md).
- Need REST endpoint shapes? Read [REST API](../spec/api/rest.md).
- Need the machine-readable contract? Read
  [authentication-overview.yaml](../spec/security/authentication-overview.yaml)
  or run `ugoite auth overview`.

## Reference: the `ugoite auth overview` command

Use this when you want the runtime snapshot instead of prose:

```bash
ugoite auth overview
```

The JSON export includes:

- mandatory authentication enforcement flags
- configured bearer and API-key provider capabilities
- identity model fields such as `user_id`, `principal_type`, and `scopes`
- supported client channels such as `backend(rest)`, `backend(mcp)`,
  `cli(via backend)`, and `frontend(via backend)`

## Source of truth

- Runtime export: `ugoite_core.auth.export_authentication_overview()`
- Documentation contract: `docs/spec/security/authentication-overview.yaml`
- Consistency test: `ugoite-core/tests/test_auth_overview_consistency.py`
