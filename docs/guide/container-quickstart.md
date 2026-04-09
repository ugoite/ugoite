# Container Quick Start

Use this guide when you want the simplest way to run the latest published
Ugoite browser experience locally. It downloads the shipped
`docker-compose.release.yaml`, prepares a small `.env` file, pulls the published
GHCR images, and starts the stack without cloning the repository or rebuilding
images from source.

This is the fastest **browser** path, but it is not the lowest-overhead path:
it still needs Docker, published image pulls, and a two-service stack. If you
want the lightest local-first start, use the [CLI Guide](cli.md) in `core`
mode instead.

For local development from source, keep using
[Docker Compose Guide](docker-compose.md).
If you want the same published two-service topology on Kubernetes, use
[Helm Chart Guide](helm-chart.md).

## Quick start

Create a small working directory, download the release compose file, and
generate an `.env` file with install-specific auth values plus an explicit local
demo-login opt-in:

```bash
mkdir -p ugoite-release
cd ugoite-release
curl -fsSLO "https://github.com/ugoite/ugoite/releases/latest/download/docker-compose.release.yaml"
python3 - <<PY > .env
import secrets

demo_mode = "mock-oauth"
signing_kid = "release-compose-local-v1"
signing_secret = secrets.token_urlsafe(32)
proxy_token = secrets.token_urlsafe(32)

print("UGOITE_VERSION=stable")
print("UGOITE_SPACES_DIR=./spaces")
print("UGOITE_FRONTEND_PORT=3000")
print("UGOITE_BACKEND_PORT=8000")
print(f"UGOITE_DEV_AUTH_MODE={demo_mode}")
print("UGOITE_DEV_USER_ID=dev-local-user")
print(f"UGOITE_DEV_SIGNING_KID={signing_kid}")
print(f"UGOITE_DEV_SIGNING_SECRET={signing_secret}")
print(f"UGOITE_AUTH_BEARER_SECRETS={signing_kid}:{signing_secret}")
print(f"UGOITE_AUTH_BEARER_ACTIVE_KIDS={signing_kid}")
print(f"UGOITE_DEV_AUTH_PROXY_TOKEN={proxy_token}")
PY
mkdir -p ./spaces
chmod 0777 ./spaces
```

The shipped manifest itself now stays on the safer `passkey-totp` default and
requires operator-supplied auth values. The example above explicitly opts into
loopback-only `mock-oauth` for the published local demo flow.

If you do not have `python3` locally, generate equivalent random values with
your preferred secret tool before writing `.env`.

Pull and start the published stack:

```bash
docker compose -f docker-compose.release.yaml pull
docker compose -f docker-compose.release.yaml up -d
```

If the stack does not start cleanly, ports are already occupied, or the browser
cannot reach the backend, follow
[Compose Startup and Connectivity Troubleshooting](troubleshooting-compose-startup.md)
before debugging login/auth behavior.

The compose file pulls these canonical published images:

- `ghcr.io/ugoite/ugoite/backend:${UGOITE_VERSION}`
- `ghcr.io/ugoite/ugoite/frontend:${UGOITE_VERSION}`

Then open:

- Frontend UI login: http://localhost:3000/login
- Backend API: http://localhost:8000

Click **Continue with Local Demo Login** to reach `/spaces`. That button starts
the local demo login path (`mock-oauth`), so no external OAuth provider is
involved. The shipped compose file bootstraps the `default` space at startup so
the first browser and CLI session both have a ready workspace. The reserved
`admin-space` still exists for admin-only workflows, but `/spaces` keeps it in a
separate admin section so the first visible workspace path stays newcomer-friendly.
For more detail on the explicit browser login flow, see
[Local Dev Auth Login](local-dev-auth-login.md).
For the concrete post-login space -> form -> entry path, continue to
[Browser Walkthrough: First Space, Form, and Entry](browser-first-entry.md).

This published quick start intentionally differs from `mise run dev`: the
manifest defaults to `passkey-totp` with operator-supplied auth material, while
the example above explicitly opts into `mock-oauth` for a loopback-only browser
demo. Source development still keeps `passkey-totp` as the default so
contributors exercise the explicit passkey + 2FA flow.

## Why does release compose set `UGOITE_ALLOW_REMOTE=true`?

The shipped `docker-compose.release.yaml` runs the frontend and backend as two
separate containers. The frontend reaches the backend through the Compose
network at `http://backend:8000`, so the backend must accept non-loopback
traffic **inside that private container network**. That is why the published
compose file sets `UGOITE_ALLOW_REMOTE=true`.

This does **not** make the quick start publicly reachable by default. The
published host ports still bind to `127.0.0.1`, so the browser UI and backend
API remain localhost-only on the host unless you edit the compose file
yourself.

If you want stricter host exposure, remove the backend `ports:` mapping and use
the browser only through the frontend container; it will still reach
`http://backend:8000` over the Compose network. If you need a topology that
keeps `UGOITE_ALLOW_REMOTE=false`, use the CLI in `core` mode or a non-Compose
workflow where the client talks to a loopback-bound backend directly.

## Where browser-created data lives

The browser path is still local-first in practice. When you create entries
through the published UI, the backend writes that data into the host-mounted
spaces directory, not into a hosted database.

- By default, that host path is `./spaces`.
- If you override `UGOITE_SPACES_DIR`, inspect or back up that host path
  instead.
- On Linux bind mounts, keep that directory writable for the non-root backend
  image user before the first startup; the quick-start example above does this
  with `chmod 0777 ./spaces`.
- This is what "local-first" means for the published browser path: you can
  examine and copy the underlying data directory yourself.

For example, after creating content in the browser:

```bash
ls ./spaces
find ./spaces -maxdepth 2 -type f | head
```

## Next steps

- The `default` space is the starter workspace that the published quick start
  bootstraps for you after login. The reserved `admin-space` stays separate in
  the UI for admin tasks.
- Follow [Browser Walkthrough: First Space, Form, and Entry](browser-first-entry.md)
  when you want the exact post-login path through the first useful browser task.
- Read [Core Concepts](concepts.md) once you want the mental model for spaces,
  entries, forms, and search behind the browser workflow you just started. If you skipped the primer earlier, do that before exploring more of the UI or the deeper docs.
- After that first browser-created entry, inspect `./spaces` (or your overridden
  `UGOITE_SPACES_DIR`) to see where the data now lives on the host.
- Switch to the [CLI Guide](cli.md) when you want a lighter terminal-first
  workflow, or to the [Docker Compose Guide](docker-compose.md) when you want
  the full contributor stack from source.
- If the published stack starts in a confusing partial state, use
  [Compose Startup and Connectivity Troubleshooting](troubleshooting-compose-startup.md).

To stop the stack:

```bash
docker compose -f docker-compose.release.yaml down --remove-orphans
```

## Environment Variables

These are the supported release-compose environment variables for the shipped
`docker-compose.release.yaml` quick start:

| Variable | Default | Purpose |
| --- | --- | --- |
| `UGOITE_VERSION` | required | Published image tag selector. Set it to `stable` or `latest` for the newest stable release, `alpha` or `beta` for the newest prerelease channel, or an exact version such as `0.0.1` to pin the stack. |
| `UGOITE_SPACES_DIR` | `./spaces` | Host path mounted into `/data` so the backend keeps the local-first storage directory outside the container. |
| `UGOITE_FRONTEND_PORT` | `3000` | Host port exposed for the frontend UI. |
| `UGOITE_BACKEND_PORT` | `8000` | Host port exposed for the backend API. |
| `UGOITE_DEV_AUTH_MODE` | `passkey-totp` | Dev login mode inside the shipped manifest. Set it to `mock-oauth` only for an explicit local demo flow. |
| `UGOITE_DEV_USER_ID` | required | Username/user id for the explicit login flow you enable. The quick-start example above sets `dev-local-user` explicitly. |
| `UGOITE_DEV_SIGNING_KID` | `release-compose-local-v1` | Key id paired with your install-specific bearer signing material. |
| `UGOITE_DEV_SIGNING_SECRET` | required unique value | Secret used to mint dev bearer tokens for this install. |
| `UGOITE_AUTH_BEARER_SECRETS` | required unique value | Bearer verification secret set accepted by the backend. For the quick start, reuse the same signing kid + secret pair. |
| `UGOITE_AUTH_BEARER_ACTIVE_KIDS` | `release-compose-local-v1` | Active bearer-token key ids exposed to the backend. |
| `UGOITE_DEV_AUTH_PROXY_TOKEN` | required unique value | Shared token between frontend and backend so `/login` can reach the explicit auth endpoints. |

The shipped compose file keeps `BACKEND_URL=http://backend:8000` fixed inside
the Compose network. By default it stays on `passkey-totp`; the quick-start
example above opts into `mock-oauth` only after generating install-specific
secrets. For a broader mode-by-mode reference, see
[Environment Variable Matrix](env-matrix.md).

## Version selectors

Choose the release channel that matches your goal:

- `stable` or `latest` for the newest stable release
- `alpha` for the newest alpha prerelease
- `beta` for the newest beta prerelease
- an exact version such as `0.0.1`, `0.0.1-beta.7`, or `0.0.1-alpha.3` when
  you need a specific published build

## Notes

- By default, the release compose file keeps data on the host under `./spaces`
  to preserve the local-first storage model.
- The shipped manifest itself stays on `passkey-totp` and refuses repository-known
  auth secrets. The quick-start example above explicitly opts into loopback-only
  `mock-oauth` with install-specific signing and proxy values.
- The frontend container talks to the backend through the Compose network via
  `http://backend:8000`, which is why the shipped backend environment keeps
  `UGOITE_ALLOW_REMOTE=true` inside the container network even though host
  access still stays on `127.0.0.1`.
- If you want source-mounted development containers instead, use
  `docker-compose.yaml` and build locally.
