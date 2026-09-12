---
title: "Configure"
description: Environment variables and configuration entry points.
sidebar:
  order: 3
---

Configuration starts after selecting a runtime shape in
[Install and Deploy](install-deploy.md). The
[environment matrix](../guide/deploy/env-matrix.md) is the operator authority
for individual variables.

## What to configure first

Public origin, WebAuthn relying-party ID, Space storage locations, Node
control-store location, and the node secret source. HTTPS is required for a
production remote deployment.

## What became durable?

Configuration selects where durable state lives; it is not itself Space content.
Preserve the Space prefix, the control-store prefix, and the node secret
separately.

## Related

- [Environment matrix](../guide/deploy/env-matrix.md)
- [Storage and Recovery](storage-recovery.md)
