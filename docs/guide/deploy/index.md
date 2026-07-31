---
title: "Deploy Ugoite"
description: Choose a deployment shape and configure its public origin, storage, and secrets.
sidebar:
  label: "Overview"
  order: 1
---

Choose the deployment shape before changing individual environment variables.
All supported shapes run the same Rust server. They keep Space storage
operator-owned, while Node control state and the node secret remain separate
recovery inputs.

## Choose a deployment shape

- **Try the release image:** start with the
  [container quick start](../start/container-quickstart.md).
- **Build from source:** use [Docker Compose](docker-compose.md) when working on
  the repository or the runtime image.
- **Run on Kubernetes:** install the [Helm chart](helm-chart.md) with one PVC
  and one node-local replica.
- **Tune a deployment:** use the [environment variables](env-matrix.md) after
  selecting the runtime shape.

## Configuration that matters first

The public HTTPS origin and WebAuthn relying-party ID must agree before the
first Passkey is registered. The node encryption root must be generated once,
preserved across restarts, and kept separate from login credentials.

## Storage and recovery shape

The default local layout places Space storage and the default Node control store
under `UGOITE_ROOT` (the supplied containers mount this at `/data`). That makes
one filesystem snapshot sufficient for those two storage inputs, but only when
the node secret is preserved with it as well. `UGOITE_NODE_SECRET_KEY` and a
separately mounted `UGOITE_NODE_SECRET_FILE` are not automatically part of the
`/data` snapshot.

When `UGOITE_NODE_CONTROL_URI` points at another OpenDAL backend, back up that
complete control-store prefix separately from every Space prefix. Space storage
is the portable move unit; Node control state stays node-local.
