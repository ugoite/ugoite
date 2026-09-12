---
title: "Operate Ugoite"
description: Install, configure, secure, recover, and troubleshoot a Ugoite deployment.
sidebar:
  label: "Overview"
  order: 1
---

Operate Ugoite is for people who keep a deployment running. It collects install,
configuration, identity, storage, diagnostics, upgrade, and troubleshooting
procedures without requiring Architecture docs first.

## Procedures

- [Install and Deploy](install-deploy.md): choose the runtime shape.
- [Configure](configure.md): environment variables and entry points.
- [Identity and Access](identity-access.md): login, device auth, recovery, and
  membership.
- [Storage and Recovery](storage-recovery.md): moves, verification, and the
  three recovery inputs.
- [Health and Diagnostics](health-diagnostics.md): health checks and safe logs.
- [Upgrade and Compatibility](upgrade-compatibility.md): upgrades without
  breaking portable Spaces.
- [Troubleshooting](troubleshooting.md): symptom-first fixes.

## Recovery inputs

A portable recovery preserves three things separately: the Space prefix, the
Node control-store prefix, and the node secret. A `/data` directory copy alone
is not a complete recovery set when the control store or secret lives elsewhere.

Detailed legacy procedures remain in [Deploy Ugoite](../guide/deploy/index.md),
[Operate Ugoite](../guide/operate/index.md), and
[Troubleshoot Ugoite](../guide/troubleshoot/index.md) until they are folded
here.
