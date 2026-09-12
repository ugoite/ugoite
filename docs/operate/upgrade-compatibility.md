---
title: "Upgrade and Compatibility"
description: Upgrade safely without breaking portable Spaces.
sidebar:
  order: 7
---

Upgrades keep portable Spaces readable. The
[Space compatibility contract](../architecture/contracts/space-compatibility.md)
and the [release contract](../architecture/release/release-contract.md) are the
authorities; this page is only the operator entry point.

## Rules

- Preserve each configured Space prefix, the control-store prefix, and the node
  secret separately across upgrades.
- Ordinary pushes do not update release metadata; version preparation, candidate
  verification, and promotion are explicit steps.

## What became durable?

Unchanged Space content across the upgrade. Upgrade tooling state is not Space
authority.

## Related

- [Space compatibility contract](../architecture/contracts/space-compatibility.md)
- [Release contract](../architecture/release/release-contract.md)
- [Current Product and Target State](../vision/current-and-target.md)
