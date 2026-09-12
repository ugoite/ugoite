---
title: "Health and Diagnostics"
description: Keep a running node healthy and read its logs safely.
sidebar:
  order: 6
---

Check health before diagnosing content symptoms.

## Procedures

- [Health checks](../guide/operate/server/backend-healthcheck.md) for readiness
  and liveness.
- [Operations runbook](../guide/operate/server/operations.md) for daily server
  work.
- [Log redaction](../guide/troubleshoot/log-redaction.md) before sharing logs;
  secrets and credentials must not leave the node.

## What became durable?

Health output and logs are diagnostics. They never stand in for Space authority
or recovery inputs.

## Related

- [Troubleshooting](troubleshooting.md)
