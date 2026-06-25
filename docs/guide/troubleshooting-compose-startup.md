# Troubleshooting Compose startup

## Find the URL

```bash
docker compose port ugoite 8000
```

Source Compose uses a random loopback port; release Compose uses `${UGOITE_PORT:-8000}`.

## Inspect an exit

```bash
docker compose ps
docker compose logs ugoite
```

Check image/build completion, required `UGOITE_VERSION`, and write permission on the `/data` mount for the non-root user.

## Login failure

Mock OAuth requires `UGOITE_DEV_AUTH_MODE=mock-oauth` and the matching `UGOITE_BOOTSTRAP_TOKEN`. Passkey/TOTP `/auth/login` is unavailable in this release.

## Missing data

Confirm the host directory is mounted at `/data` and `UGOITE_ROOT=/data`. There is no separate authoritative database or frontend container.
