---
title: "Configuration"
description: Environment variables and configuration entry points.
sidebar:
  order: 5
---

Operators configure deployments through environment variables after selecting a
runtime shape. The [environment matrix](../guide/deploy/env-matrix.md) is the
authority; this page only names the entry points.

## Entry points

Public origin and WebAuthn relying-party ID, Space storage locations, Node
control-store location, and the node secret source. HTTPS is required for
production remote deployments.

## What to preserve

The Space prefix, the control-store prefix, and the node secret separately. A
data-directory copy alone is incomplete when the control store or secret lives
elsewhere.

## Related

- [Configure](../operate/configure.md)
- [Install and Deploy](../operate/install-deploy.md)
