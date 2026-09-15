---
title: "Health and Diagnostics"
description: Check readiness and collect safe diagnostics from a running node.
sidebar:
  order: 6
---

Check the smallest observable boundary before diagnosing a Space or identity
symptom. Health output and logs are evidence about a running node, not a second
Knowledge authority.

## Check readiness

For a local server, query the unauthenticated health endpoint:

```bash
curl --fail "http://127.0.0.1:${UGOITE_PORT:-8000}/health"
```

For source Compose, resolve its loopback port first:

```bash
docker compose port ugoite 8000
docker compose ps
docker compose logs ugoite
```

HTTP `200` confirms that the process accepts requests. It does not validate
every Space, credential, or storage backend.

## Inspect a running deployment

1. Confirm the container or pod is running and its configured port is reachable.
2. Check `/health` and the browser/API origin.
3. Read bounded logs for startup, mount, origin, and storage errors.
4. Redact secrets before sharing any output.
5. If the node is healthy but a Knowledge task fails, continue with
   [Troubleshooting](troubleshooting.md).

The supplied release container runs as a non-root user. A permission error on
`/data` or a configured storage backend must be fixed at the deployment
boundary; do not make Space files world-writable.

## Safe log handling

Never share authorization headers, setup or invitation URLs, access or refresh
credentials, encryption keys, session cookies, recovery tokens/codes, TOTP
secrets, complete Entry bodies, Asset bytes, or unbounded SQL results. Prefer
operation name, request ID, Space UID, status, duration, and bounded error
metadata.

Owner recovery responses are not cacheable. A pending audit delivery is a
diagnostic state and must not cause a one-time token or code to be replayed.

## What remains durable?

Health output, logs, container state, and derived index status are diagnostics.
They do not replace the Space prefix, its append-only history, the Node
control-store prefix, or the node secret. Preserve those recovery inputs before
restarting, moving, or cleaning up a deployment.

## Related

- [Install and Deploy](install-deploy.md)
- [Storage and Recovery](storage-recovery.md)
- [Troubleshooting](troubleshooting.md)
