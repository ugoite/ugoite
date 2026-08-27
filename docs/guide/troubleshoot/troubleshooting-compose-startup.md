---
title: "Troubleshooting Compose startup"
sidebar:
  order: 2
---

> Release note: Passkey setup, passwordless browser login, and the explicit
> recovery-only TOTP setup for Account Self-Recovery are supported v0.1
> behavior. Invitation-gated OIDC authentication and linking are also
> supported when a provider is configured.

Use these checks when the Compose service does not start, cannot be reached,
rejects login, or appears to lose mounted data.

## Find the URL

```bash
docker compose port ugoite 8000
```

Source Compose uses a random loopback port; release Compose uses
`${UGOITE_PORT:-8000}`.

## Inspect an exit

```bash
docker compose ps
docker compose logs ugoite
```

Check image/build completion, required `UGOITE_VERSION`, and write permission on
the `/data` mount for the non-root user.

## Login failure

First startup prints a one-use setup URL. If Passkey registration reports an
origin or RP mismatch, verify that `UGOITE_PUBLIC_ORIGIN` exactly matches the
browser origin and `UGOITE_WEBAUTHN_RP_ID` matches its host, then restart with
an empty data root before any credential has been registered.

## Missing data

Confirm the host directory is mounted at `/data` and `UGOITE_ROOT=/data`. There
is no separate authoritative database or frontend container.
