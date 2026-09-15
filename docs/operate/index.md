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

## Safe operating sequence

1. Choose a deployment shape in [Install and Deploy](install-deploy.md).
2. Configure the public origin, storage locations, control store, and node
   secret in [Configure](configure.md).
3. Complete the supported bootstrap and access flow in [Identity and
   Access](identity-access.md).
4. Verify health, a representative Space read/write/restore path, and the
   recovery inputs after any deployment change.

If a failure interrupts this sequence, stop writes and use
[Storage and Recovery](storage-recovery.md) before attempting cleanup. The
Space prefix remains the Knowledge authority; diagnostics, sessions, and
derived indexes do not replace it.
