# Compose Startup and Connectivity Troubleshooting

Use this guide when the container stack does not start cleanly enough to reach
the normal login flow. It covers the earlier failure modes that happen before
[Troubleshooting Unauthorized Spaces Page](troubleshooting-unauthorized-spaces.md)
becomes relevant.

Use the same Compose command prefix that you used to start the stack:

- Source checkout: `docker compose`
- Published release quick start: `docker compose -f docker-compose.release.yaml`

## 1. Check whether ports 3000 and 8000 are already occupied

```bash
ss -ltnp | grep -E ':(3000|8000)\s'
```

- If another local process already owns one of these ports, stop that process
  before retrying Ugoite.
- For the published quick start you can also change `UGOITE_FRONTEND_PORT` and
  `UGOITE_BACKEND_PORT` in `.env`, then run the same release compose commands
  again.

## 2. Confirm backend readiness before debugging the frontend

```bash
curl -sS http://localhost:8000/health
```

Expected output:

```json
{"status":"ok"}
```

If this health check fails, continue with the backend log checks below. For the
health endpoint contract, see [Backend Healthcheck](backend-healthcheck.md).

## 3. Inspect frontend and backend logs together

Check the current service state first:

```bash
docker compose ps
```

Then inspect both services:

```bash
docker compose logs --tail=100 backend
docker compose logs --tail=100 frontend
```

If you are using the published release quick start, replace those commands with
`docker compose -f docker-compose.release.yaml ...`.

Focus on these two checks:

- The backend should bind successfully and answer `GET /health`.
- The frontend container must reach the backend through the Compose network at
  `http://backend:8000`, not through `http://localhost:8000`.

If the browser shows a blank page or repeated API failures, inspect the
frontend logs for proxy errors and the backend logs for startup exceptions at
the same time.

## 4. Validate that the local spaces directory is writable

The Compose stack keeps local-first data on the host through the spaces mount.
Use the same host path that your stack is configured to mount:

- Source checkout: `./spaces`
- Published release quick start: `${UGOITE_SPACES_DIR:-./spaces}`

One quick write test:

```bash
SPACE_PATH="${UGOITE_SPACES_DIR:-./spaces}"
mkdir -p "$SPACE_PATH"
sudo chown 10001:10001 "$SPACE_PATH"
chmod 0750 "$SPACE_PATH"
touch "$SPACE_PATH/.ugoite-write-test" && rm "$SPACE_PATH/.ugoite-write-test"
```

The published release quick start uses a non-root backend image. On Linux bind
mounts, a host directory that still has the usual `0755` mode can reject writes
from that container user even when your shell user created the directory.

If you cannot change ownership but your host has ACL tooling, try a narrower
rule such as `setfacl -m u:10001:rwx "$SPACE_PATH"` before broadening the mode.
Use a world-writable fallback only as a last resort:

```bash
chmod 0777 "$SPACE_PATH"
touch "$SPACE_PATH/.ugoite-write-test" && rm "$SPACE_PATH/.ugoite-write-test"
```

If the write test still fails after those steps, fix the directory ownership or
permissions on the host before retrying the stack.

## 5. Reset stale services, networks, and partial startup state

Repeated interrupted runs can leave orphaned containers or a half-started
network behind. Reset the stack and start it again from a clean Compose state:

```bash
docker compose down --remove-orphans
docker compose up --build
```

For the published release quick start:

```bash
docker compose -f docker-compose.release.yaml down --remove-orphans
docker compose -f docker-compose.release.yaml pull
docker compose -f docker-compose.release.yaml up -d
```

If you intentionally want a full local data reset for the source checkout, you
can remove `./spaces` after the stack is down and then start again.

## 6. Continue to auth-specific troubleshooting only after startup is healthy

Once the backend health check succeeds, the frontend loads, and the services can
reach each other, continue with
[Troubleshooting Unauthorized Spaces Page](troubleshooting-unauthorized-spaces.md)
if `/spaces` still fails after login.
