# Container Quick Start

Use this guide when you want the simplest way to run the latest published
Ugoite browser experience locally. It downloads the shipped
`docker-compose.release.yaml`, prepares a small `.env` file, pulls the published
GHCR images, and starts the stack without cloning the repository or rebuilding
images from source.

For local development from source, keep using
[Docker Compose Guide](docker-compose.md).
If you want the same published two-service topology on Kubernetes, use
[Helm Chart Guide](helm-chart.md).

## Quick start

Create a small working directory, download the release compose file, and add the
runtime environment values:

```bash
mkdir -p ugoite-release
cd ugoite-release
curl -fsSLO "https://github.com/ugoite/ugoite/releases/latest/download/docker-compose.release.yaml"
cat > .env <<EOF
UGOITE_VERSION=stable
UGOITE_SPACES_DIR=./spaces
UGOITE_FRONTEND_PORT=3000
UGOITE_BACKEND_PORT=8000
UGOITE_DEV_USER_ID=dev-local-user
UGOITE_DEV_AUTH_PROXY_TOKEN=release-compose-auth-proxy
EOF
mkdir -p ./spaces
```

Pull and start the published stack:

```bash
docker compose -f docker-compose.release.yaml pull
docker compose -f docker-compose.release.yaml up -d
```

The compose file pulls these canonical published images:

- `ghcr.io/ugoite/ugoite/backend:${UGOITE_VERSION}`
- `ghcr.io/ugoite/ugoite/frontend:${UGOITE_VERSION}`

Then open:

- Frontend UI login: http://localhost:3000/login
- Backend API: http://localhost:8000

Click **Continue with Mock OAuth** to reach `/spaces`. The shipped compose file
bootstraps the `default` space at startup so the first browser and CLI session
both have a ready workspace. For more detail on the explicit browser login
flow, see [Local Dev Auth Login](local-dev-auth-login.md).

This published quick start intentionally differs from `mise run dev`: it
defaults to `mock-oauth` so first-time browser evaluators can reach `/spaces`
with fewer steps, while source development keeps `passkey-totp` as the default
so contributors exercise the explicit passkey + 2FA flow.

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
- This is what "local-first" means for the published browser path: you can
  examine and copy the underlying data directory yourself.

For example, after creating content in the browser:

```bash
ls ./spaces
find ./spaces -maxdepth 2 -type f | head
```

## Next steps

- The `default` space is the starter workspace that the published quick start
  bootstraps for you after login.
- Try creating one plain Markdown entry in that space first. You do **not** need
  to define a Form before the first note.
- After that first browser-created note, inspect `./spaces` (or your overridden
  `UGOITE_SPACES_DIR`) to see where the data now lives on the host.
- Read [Core Concepts](concepts.md) next if you want the mental model for
  spaces, entries, forms, and search before exploring more of the UI.
- Switch to the [CLI Guide](cli.md) when you want a lighter terminal-first
  workflow, or to the [Docker Compose Guide](docker-compose.md) when you want
  the full contributor stack from source.

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
| `UGOITE_DEV_USER_ID` | `dev-local-user` | Mock OAuth user id created by the shipped release compose login flow. |
| `UGOITE_DEV_AUTH_PROXY_TOKEN` | `release-compose-auth-proxy` | Shared token between frontend and backend so `/login` works out of the box in the published quick start. |

The shipped compose file keeps `BACKEND_URL=http://backend:8000` fixed inside
the Compose network, and it pre-wires the signing/bearer settings needed for
the explicit `mock-oauth` browser login flow. For a broader mode-by-mode
reference, see [Environment Variable Matrix](env-matrix.md).

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
- The published quick start binds both services to `127.0.0.1`, wires the dev
  auth proxy token between frontend and backend, bootstraps the `default`
  space, and enables explicit `mock-oauth` browser login so `/login` works
  without editing the compose file.
- The frontend container talks to the backend through the Compose network via
  `http://backend:8000`, which is why the shipped backend environment keeps
  `UGOITE_ALLOW_REMOTE=true` inside the container network even though host
  access still stays on `127.0.0.1`.
- If you want source-mounted development containers instead, use
  `docker-compose.yaml` and build locally.
