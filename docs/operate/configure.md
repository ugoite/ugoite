---
title: "Configure"
description: Configure origins, storage, control state, and secrets for a deployment.
sidebar:
  order: 3
---

Configuration selects where a deployment runs; it does not become Space
Knowledge. Start after choosing a shape in [Install and Deploy](install-deploy.md).

## Outcome

The server's public origin, WebAuthn relying-party identity, storage locations,
and encryption root agree before the first credential is registered.

## Configure the first start

Set the externally visible HTTPS origin and its host before opening the setup
URL:

```bash
export UGOITE_PUBLIC_ORIGIN=https://ugoite.example.com
export UGOITE_WEBAUTHN_RP_ID=ugoite.example.com
```

For the integrated browser image, keep `UGOITE_API_BASE_URL` aligned with the
public API path. Configure the Space root with `UGOITE_ROOT`; if Node control
state is stored elsewhere, set `UGOITE_NODE_CONTROL_URI` to that complete
OpenDAL prefix.

Provide the node encryption root through `UGOITE_NODE_SECRET_KEY` or a mounted
`UGOITE_NODE_SECRET_FILE`. This secret is not a login credential and must be
preserved separately from the control-store namespace. Do not rotate or discard
it during a routine restart.

## Change configuration safely

1. Stop or quiesce writes before moving a Space or its control store.
2. Back up every configured prefix and the node secret.
3. Change one boundary at a time, then start the server.
4. Verify `/health`, login, Space listing, and a representative read/write/
   restore path.

The exact variable names, defaults, and deployment entry points are collected
in [Reference / Configuration](../reference/configuration.md). REST endpoint
and schema facts remain in the server implementation and
[`/openapi.json`](../reference/rest.md).

## What became durable?

The configured location determines where durable Space data and node-local
control data can be recovered. The environment itself is deployment state; it
does not create a second Knowledge authority.

## If it fails

- Origin or RP mismatch: correct the public origin and RP ID before a new
  credential ceremony.
- Space missing after a change: restore the complete Space prefix and confirm
  the configured path or backend, rather than rebuilding an index.
- Login state missing after restart: restore the same control-store prefix and
  node secret, then retry the session.

See [Storage and Recovery](storage-recovery.md) for restore verification and
[Troubleshooting](troubleshooting.md) for symptom-first diagnosis.

## Related

- [Install and Deploy](install-deploy.md)
- [Identity and Access](identity-access.md)
- [Configuration Reference](../reference/configuration.md)
