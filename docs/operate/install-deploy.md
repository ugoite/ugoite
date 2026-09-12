---
title: "Install and Deploy"
description: Choose a deployment shape and start the Rust server.
sidebar:
  order: 2
---

Choose the deployment shape before changing individual environment variables.
All supported shapes run the same Rust server with operator-owned Space storage.

## Deployment shapes

- Try the release image with the
  [container quick start](../guide/start/container-quickstart.md).
- Build from source with [Docker Compose](../guide/deploy/docker-compose.md)
  when working on the repository or runtime image.
- Run on Kubernetes with the [Helm chart](../guide/deploy/helm-chart.md) using
  one PVC and one node-local replica.
- Tune afterwards with [Configure](configure.md).

## What became durable?

The deployment choice itself is operator state. Space content stays portable
regardless of shape; Node control state and the node secret remain separate
recovery inputs. See [Storage and Recovery](storage-recovery.md).

## Related

- [Deploy Ugoite](../guide/deploy/index.md)
- [Operate overview](index.md)
