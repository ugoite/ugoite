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

## Find the procedure

- Choose a deployment shape in [Deploy Ugoite](../guide/deploy/index.md).
- Keep a node healthy in [Operate Ugoite](../guide/operate/index.md).
- Diagnose the shortest path in
  [Troubleshoot Ugoite](../guide/troubleshoot/index.md).
- Review environment variables in the
  [environment matrix](../guide/deploy/env-matrix.md).

## Recovery inputs

A portable recovery preserves three things separately: the Space prefix, the
Node control-store prefix, and the node secret. A `/data` directory copy alone
is not a complete recovery set when the control store or secret lives elsewhere.
See [Operate Ugoite](../guide/operate/index.md) for the full boundary.

Troubleshooting pages are organized symptom-first. Later steps will merge the
current deploy, operate, and troubleshoot guides here without changing their
procedures.
