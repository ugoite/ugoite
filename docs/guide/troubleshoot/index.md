---
title: "Troubleshoot Ugoite"
description: Diagnose startup, health, authentication, logging, and storage problems in the shortest useful order.
sidebar:
  label: "Overview"
  order: 1
---

Troubleshooting starts with the smallest observable boundary and moves inward.
Do not delete Space data or rotate the node secret while the cause is still
unknown.

## Diagnostic order

1. Check the [server health endpoint](../operate/server/backend-healthcheck.md).
2. If the process is not ready, inspect
   [Compose startup](troubleshooting-compose-startup.md).
3. If requests are rejected, read
   [unauthorized Spaces](troubleshooting-unauthorized-spaces.md) and the
   [authentication guide](../operate/auth/auth-overview.md).
4. If logs expose too much or too little, use [log redaction](log-redaction.md).
5. Only then investigate
   [storage cleanup](../operate/storage/storage-cleanup.md). For an old or
   incomplete Space layout, preserve the complete prefix and report the
   explicit unsupported-layout error; do not rewrite it in place.

The complete Space prefix, the configured Node control-store prefix, and the
node encryption root are separate recovery inputs; keep all of them intact
while diagnosing a deployment.
