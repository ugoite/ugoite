---
title: "Migration status"
description: Which specification domains are canonical in Mitase and which remain in docs/spec.
---

Mitase owns migrated specifications. This page locates the active sources.

## Cross-surface preflight authority map

The canonical requirement, feature, philosophy, and policy graph lives in
[Mitase](https://github.com/ugoite/ugoite/tree/main/docs/mitase). REQ-OPS-003
checks that no requirement records remain in `docs/spec/requirements`.
`docs/spec/ui` owns UI contracts for pages and components. The
[architecture](../architecture/index.md) explains the implementation
boundaries.

When migrating a record, preserve its meaning and any verification gaps,
change the canonical specification artifact, and remove the duplicate record.
`scripts/mitase check .` checks the current graph. Earlier registries and
migration decisions remain in Git history.
