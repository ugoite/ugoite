---
title: "Release architecture"
description: The current v0.1 capability boundary and the remaining consolidation work.
sidebar:
  label: "Release"
  order: 4
---

This group separates what the current release implements from the architecture
that is still intentionally future scope. It is the right place to check whether
a product or support claim is safe to make.

- [Current release scope](release-scope.md) lists included, limited, and
  unavailable capabilities.
- [Versioning and release contract](release-contract.md) defines preparation,
  candidate verification, and buildless promotion.
- [Release platform support](platform-support.md) defines the Tier 1, Tier 2,
  and unsupported artifact targets.
- [Release planner ref recovery](release-planner-recovery.md) defines the
  compare-before-delete procedure for an orphaned planner branch ref.
- [v0.1 Knowledge compatibility floor](v0.1-knowledge-compatibility.md) defines
  semantic recoverability and authority invariants.
- [v0.2.1 stable release note](../../version/releases/v0.2.1.md) records the
  published release and its compatibility boundary.
- [v0.2.1 operation-audit baseline](v0.2.1-operation-audit-baseline.md) records
  six independently reproduced findings and separates baseline observations from
  fix acceptance evidence.
- [v0.2.1 Remote SQL export authorization acceptance](v0.2.1-remote-export-auth.md)
  preserves the fixed-SHA security acceptance evidence for membership
  revocation and access-token refresh between export pages.
- [Release notes and historical channel records](changelog.md) point to the
  versioned stable notes and explain that beta/alpha are not active channels.
