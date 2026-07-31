---
title: Docker Compose
sidebar:
  order: 2
---

Generate an encryption root and start the source image:

```bash
export UGOITE_NODE_SECRET_KEY="$(head -c 32 /dev/urandom | base64)"
docker compose up --build -d
```

This builds the image, binds localhost port `${UGOITE_PORT:-8000}`, and mounts
the local runtime data directory from `./data` at `/data`. Read the setup URL
with `docker compose logs ugoite`.

Set `UGOITE_PUBLIC_ORIGIN` and `UGOITE_WEBAUTHN_RP_ID` to the externally visible
HTTPS origin and host before registering Passkeys. Set `UGOITE_NODE_SECRET_KEY`
to at least 32 random bytes, or adapt the deployment to mount
`UGOITE_NODE_SECRET_FILE`. This encryption root is not a login credential and
must not be stored in the Node control namespace. Preserve it in a deployment
secret across restarts; losing it makes encrypted recovery and OIDC material
unusable.
