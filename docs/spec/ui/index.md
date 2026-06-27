---
title: 'UI specifications'
---

Page YAML files describe the current Space-scoped browser routes. `page.implementation: implemented` means a matching SolidStart route exists under `frontend/src/routes`; it does not imply browser-local persistence.

The current browser is server-backed and authenticated. Shared navigation is described in `components/space-shell.yaml`; page files live under `pages/`. Route behavior, API calls, and loading/error states remain authoritative in the corresponding TSX files and tests.

Implemented page routes include Space home/dashboard, Entries and history/restore, Forms and column types, keyword/advanced search, saved SQL and query sessions, Assets, settings/storage visibility, and connection testing.

When changing a route:

1. update the TSX route and its tests;
2. update the matching page YAML route/status/components;
3. keep links between page IDs valid;
4. run the frontend/docsite checks through the root `mise` tasks.
