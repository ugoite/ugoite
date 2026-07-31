---
title: "Storage cleanup"
sidebar:
  order: 4
---

There is no supported repository task named `cleanup:*` and no command that
blindly removes old Space data.

Keep entries, forms, revision history, assets, membership data, and saved SQL.
Indexes and transient SQL sessions are derived, but remove them only through a
documented implementation path. Before cleanup, stop writes and back up the
complete Space directory.

Use `ugoite index run <space-path>` in core mode to rebuild an index rather than
deleting unknown files manually.
