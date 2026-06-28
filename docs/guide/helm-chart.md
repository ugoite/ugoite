---
title: 'Helm chart'
---

`charts/ugoite` deploys the same single image used by Docker Compose. Release publication pushes the packaged chart to `oci://ghcr.io/ugoite/charts`, and the chart defaults to the exact matching application version instead of a moving image alias.

```bash
helm upgrade --install ugoite charts/ugoite   --set auth.bootstrapToken="$(openssl rand -hex 32)"   --set auth.signingSecret="$(openssl rand -hex 32)"
```

Important values include `image.*`, `service.*`, `persistence.*`, `bootstrapDefaultSpace`, `auth.*`, and `extraEnv`. Templates require unique bootstrap and signing secrets.

The pod runs as non-root UID/GID `10001`, drops Linux capabilities, and mounts `/data`. Change the development `mock-oauth` default before exposing a shared deployment.
