---
title: Container quick start
sidebar:
  order: 3
---

```bash
export UGOITE_VERSION=0.1.0
export UGOITE_NODE_SECRET_KEY="$(head -c 32 /dev/urandom | base64)"
docker compose -f docker-compose.release.yaml up -d
docker compose -f docker-compose.release.yaml logs ugoite
```

The local runtime data directory is `${UGOITE_DATA_DIR:-./data}`. In this
default local layout, Space storage and the default Node control store live
below that directory. The example's `UGOITE_NODE_SECRET_KEY` is supplied by the
environment, so preserve its value separately; it is not included in a
data-directory copy. Browser authentication and account-management flows are
outside the supported v0.1 quick-start contract; use CLI core mode for the
portable local-first path.

For a remote hostname, configure the public origin before first start:

```bash
export UGOITE_PUBLIC_ORIGIN=https://ugoite.example.com
export UGOITE_WEBAUTHN_RP_ID=ugoite.example.com
docker compose -f docker-compose.release.yaml up -d
```

HTTPS is required for a production remote deployment.
