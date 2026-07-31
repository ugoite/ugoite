---
title: "Deploy Ugoite"
description: Choose a deployment shape and configure its public origin, storage, and secrets.
sidebar:
  label: "Overview"
  order: 1
---

Choose the deployment shape before changing individual environment variables.
All supported shapes run the same Rust server and keep the operator-owned data
root as the authoritative boundary.

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
