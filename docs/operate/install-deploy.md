---
title: "Install and Deploy"
description: Choose a deployment shape, start the Rust server, and complete first verification.
sidebar:
  order: 2
---

Choose a deployment shape before changing individual environment variables.
Release Compose is the documented published-image procedure; source Compose and
the local source loop support repository development. The Helm chart is a Tier 1
release artifact, while Kubernetes production deployment support is not
established by artifact packaging and verification. All shapes run the same Rust
server with operator-owned Space storage.

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

The v0.1 line is published and maintained; the current published baseline is
v0.2.1. The active v0.2 direction is Product UX: make the
frozen Foundation completable, discoverable, consistent, recoverable, and
correctly documented. Knowledge-to-tools is a North Star, not a shipped
deployment capability or v0.2 acceptance claim.

## Kubernetes / Helm

The versioned Helm chart is built, linted, included in release candidates, and
verified as a Tier 1 release artifact. Those checks establish the chart package
and distribution identity; they do not establish production Kubernetes support,
provider-specific storage durability, or high-availability behavior. Kubernetes
operators must evaluate their cluster, persistence, ingress/TLS, backup, and
recovery configuration for their deployment. The chart defaults to one
replica and persistent storage; its package verification does not certify
failover or high availability.

For the current chart values and installation example, see the chart's
[README](https://github.com/ugoite/ugoite/blob/main/charts/ugoite/README.md).
Complete the setup URL, register the
initial Passkey, save the recovery codes, and register a second Passkey before
continuing. See [Identity and Access](identity-access.md) for the authentication
and recovery boundaries.

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
