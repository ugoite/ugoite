---
title: "Storage cleanup"
sidebar:
  order: 4
---

There is no supported repository task named `cleanup:*` and no command that
blindly removes old Space data.

Keep entries, forms, revision history, assets, membership data, and saved SQL.
DerivedRelations and transient SQL sessions are replaceable, but remove them
only through a documented implementation path. Before cleanup, stop writes and back up the
complete Space prefix. Node control state and the node secret are separate
recovery inputs and are not cleaned up with Space data.

Use `ugoite index run <space-path>` in core mode to rebuild AssetText rather
than deleting unknown files manually. A missing `_ugoite/derived` prefix does
not make the authoritative Space corrupt; rebuild it from current Entry
AssetReferences.
