# Container Quick Start

Use this guide when you want the simplest way to run the latest published
Ugoite browser experience locally. It downloads the shipped
`docker-compose.release.yaml`, prepares a small `.env` file, pulls the published
GHCR images, and starts the stack without cloning the repository or rebuilding
images from source.

This is the fastest **browser** path, but it is not the lowest-overhead path:
it still needs Docker and a published image pull. If you
want the lightest local-first start, use the [CLI Guide](cli.md) in `core`
mode instead.

If you're using Docker Desktop on macOS or Windows, keep the shared bind-mount
path inside the filesystem location that your platform already exposes to
containers. If you're running the stack from WSL, keep the path inside the
distro-local Linux filesystem that your WSL distro exposes to containers. The
permission repair commands below are the native Linux bind-mount path, not the
default first step on those desktop platforms.

For local development from source, keep using
[Docker Compose Guide](docker-compose.md).
The published Compose path is a single Rust server image that serves both the
API and the built browser assets.

## Quick start

Create a small working directory, download the release compose file, and
generate an `.env` file with install-specific auth values plus an explicit local
demo-login opt-in:

```bash
mkdir -p ugoite-release
cd ugoite-release
curl -fsSLO "https://github.com/ugoite/ugoite/releases/latest/download/docker-compose.release.yaml"
signing_kid="release-compose-local-v1"
signing_secret="$(openssl rand -base64 32 | tr -d '\n')"
cat > .env <<EOF
UGOITE_VERSION=stable
UGOITE_SPACES_DIR=./spaces
UGOITE_PORT=8000
UGOITE_DEV_AUTH_MODE=mock-oauth
UGOITE_DEV_USER_ID=dev-local-user
UGOITE_DEV_SIGNING_KID=${signing_kid}
UGOITE_DEV_SIGNING_SECRET=${signing_secret}
UGOITE_AUTH_BEARER_SIGNING_SECRETS=${signing_kid}:${signing_secret}
UGOITE_AUTH_BEARER_ACTIVE_KIDS=${signing_kid}
EOF
mkdir -p ./spaces
if command -v setfacl >/dev/null 2>&1; then
  setfacl -m u:10001:rwx,d:u:10001:rwx ./spaces
else
  sudo chown "$(id -u)":10001 ./spaces
  chmod 0770 ./spaces
fi
```

The shipped manifest now uses the implemented `mock-oauth` local demo flow by default. Keep install-specific auth values for any shared or long-lived environment. If `openssl` is unavailable, generate an equivalent high-entropy secret with your preferred local secret tool before writing `.env`.

The published backend container runs as uid/gid `10001`. Prefer an ACL when the
host supports it, because that keeps the host user in control of `./spaces`
while still granting the published backend image write access. If ACL tooling is
not available, keep your current user as the owner and grant gid `10001` write
access instead. Keep `chmod 0777` as a last-resort troubleshooting step only.

Pull and start the published stack:

```bash
docker compose -f docker-compose.release.yaml pull
docker compose -f docker-compose.release.yaml up -d
```

If the stack does not start cleanly, ports are already occupied, or the browser
cannot reach the server, follow
[Compose Startup and Connectivity Troubleshooting](troubleshooting-compose-startup.md)
before debugging login/auth behavior.

The compose file pulls this canonical published image:

- `ghcr.io/ugoite/ugoite:${UGOITE_VERSION}`

Then open:

- Browser UI login: http://localhost:8000/login
- Backend API: http://localhost:8000/api

Click **Continue with Local Demo Login** to reach `/spaces`. That button starts
the local demo login path (`mock-oauth`), so no external OAuth provider is
involved. The shipped compose file bootstraps the `default` space at startup so
the first browser and CLI session both have a ready workspace. The reserved
`admin-space` still exists for admin-only workflows, but `/spaces` keeps it in a
separate admin section so the first visible workspace path stays newcomer-friendly.
For more detail on the explicit browser login flow and the canonical auth-mode
comparison, see [Local Development Authentication and Login]
(local-dev-auth-login.md).
For the concrete post-login space -> form -> entry path, continue to
[Browser Walkthrough: First Space, Form, and Entry](browser-first-entry.md).

This published quick start matches `mise run dev`: both use the implemented `mock-oauth` local demo flow so the browser path reaches `/login` and `/spaces` without an unfinished passkey/TOTP dependency.

## Where browser-created data lives

The browser path is still local-first in practice. When you create entries
through the published UI, the backend writes that data into the host-mounted
spaces directory, not into a hosted database.

- By default, that host path is `./spaces`.
- If you override `UGOITE_SPACES_DIR`, inspect or back up that host path
  instead.
- On Linux bind mounts, keep that directory writable for the non-root backend
  image user before the first startup; the quick-start example above assigns the
  published backend uid/gid (`10001:10001`) and keeps the mode at `0750`
  instead of making the directory world-writable.
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
| `UGOITE_VERSION` | required | Published image tag selector. Set it to `stable` or `latest` for the newest stable release, `alpha` or `beta` for the newest prerelease channel, or an exact published version to pin the stack. |
| `UGOITE_SPACES_DIR` | `./spaces` | Host path mounted into `/data` so the backend keeps the local-first storage directory outside the container. |
| `UGOITE_PORT` | `8000` | Host port exposed for the single Rust server, including browser UI and `/api/*`. |
| `UGOITE_DEV_AUTH_MODE` | `mock-oauth` | Dev login mode inside the shipped manifest. `passkey-totp` is planned but not implemented in the current Rust server. |
| `UGOITE_DEV_USER_ID` | required | Username/user id for the explicit login flow you enable. The quick-start example above sets `dev-local-user` explicitly. |
| `UGOITE_DEV_SIGNING_KID` | `release-compose-local-v1` | Key id paired with your install-specific bearer signing material. |
| `UGOITE_DEV_SIGNING_SECRET` | required 32-byte random secret | Secret used to mint dev bearer tokens for this install. |
| `UGOITE_AUTH_BEARER_SIGNING_SECRETS` | required 32-byte random secret | Bearer verification secret set accepted by the backend. For the quick start, reuse the same signing kid + secret pair. |
| `UGOITE_AUTH_BEARER_ACTIVE_KIDS` | `release-compose-local-v1` | Active bearer-token key ids exposed to the backend. |
By default the shipped compose file uses `mock-oauth`; keep install-specific
secrets for any shared or long-lived environment. For the canonical auth-mode
comparison, see
[Local Development Authentication and Login](local-dev-auth-login.md). For a
broader mode-by-mode reference, see [Environment Variable Matrix](env-matrix.md).

## Version selectors

Choose the release channel that matches your goal:

- `stable` or `latest` for the newest stable release
- `alpha` for the newest alpha prerelease
- `beta` for the newest beta prerelease
- an exact published version when you need a specific build, including
  prerelease tags from the same SemVer stream

## Notes

- By default, the release compose file keeps data on the host under `./spaces`
  to preserve the local-first storage model.
- The shipped manifest uses `mock-oauth` for the current Rust server and should be paired with install-specific signing and proxy values.
- If you want source-mounted development containers instead, use
  `docker-compose.yaml` and build locally.
