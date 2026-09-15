---
title: "Install and Deploy"
description: Choose a deployment shape, start the Rust server, and complete first verification.
sidebar:
  order: 2
---

Choose the supported deployment shape before changing individual environment
variables. Current v0.1 paths are release Compose, source Compose, and a local
source development loop; the checked-in Helm chart is explicitly unsupported.
All supported shapes run the same Rust server with operator-owned Space storage.

## Outcome

The server accepts a health request, serves the browser when static assets are
configured, and prints a one-use setup URL for the first Passkey ceremony.

## Before you begin

Choose release Compose for a published image, source Compose for repository
development, or the local source loop for contributor work. Decide the public
HTTPS origin and preserve the [three recovery inputs](storage-recovery.md)
before the first start.

## Release container

Use the published image and the release Compose file:

```bash
export UGOITE_VERSION=<published-version>
export UGOITE_NODE_SECRET_KEY="$(head -c 32 /dev/urandom | base64)"
docker compose -f docker-compose.release.yaml up -d
docker compose -f docker-compose.release.yaml logs ugoite
```

The default host binding is loopback on `${UGOITE_PORT:-8000}` and the data
mount is `${UGOITE_DATA_DIR:-./data}`. Set the public origin, API base URL,
WebAuthn RP ID, and durable secret before registering credentials; see
[Configure](configure.md).

## Source Compose

Build the current repository image when developing the server or runtime image:

```bash
export UGOITE_NODE_SECRET_KEY="$(head -c 32 /dev/urandom | base64)"
docker compose up --build -d
docker compose ps
docker compose logs ugoite
```

Source Compose binds the configurable loopback port, defaulting to
`${UGOITE_PORT:-8000}`. Resolve the active mapping with
`docker compose port ugoite 8000`. The browser remains server-backed; this
shape does not add browser-local persistence.

## Version boundary

The v0.1 line is published and maintained; this page describes the current
v0.1.1 product boundary. The active v0.2 direction is Product UX: make the
frozen Foundation completable, discoverable, consistent, recoverable, and
correctly documented. Knowledge-to-tools is a North Star, not a shipped
deployment capability or v0.2 acceptance claim.

## Kubernetes / Helm

The checked-in Helm chart is an unsupported placeholder, not a current v0.1
deployment path. Its chart metadata and startup notes are not an operator
contract, and the old local-demo login wording must not be used for a deployed
node. Do not install it for production or treat its PVC and replica settings as
supported behavior.

For Kubernetes today, use a supported container deployment procedure managed by
the operator, or wait for the chart implementation and its verification
contract to be completed. When that work lands, update this section together
with the chart; do not restore a separate legacy deployment guide.

## Source development deployment

For a local server, frontend, and docsite development loop, use
[Development Setup](../develop/development-setup.md). It starts the Rust server
with a generated local node secret and does not require a production deployment.

## Verify the result

```bash
curl --fail "http://127.0.0.1:${UGOITE_PORT:-8000}/health"
```

Complete the printed setup URL, register the initial Passkey, save the recovery
codes, and register a second Passkey. Then continue with
[Identity and Access](identity-access.md) and the
[Quickstart](../get-started/quickstart.mdx).

## What became durable?

The deployment choice itself is operator state. Space content stays portable
regardless of shape; Node control state and the node secret remain separate
recovery inputs. See [Storage and Recovery](storage-recovery.md).

## Related

- [Configure](configure.md)
- [Storage and Recovery](storage-recovery.md)
- [Troubleshooting](troubleshooting.md)
