---
title: 'UI specifications'
---

Page YAML files describe the current Space-scoped browser routes. `page.title` is route/document metadata and does not by itself require a visible heading. `page.implementation: implemented` means a matching SolidStart route exists under `frontend/src/routes`; it does not imply browser-local persistence.

The current browser is server-backed and authenticated. Shared navigation and the Browser Konase panel reference are described in `components/space-shell.yaml`; GlobalShell's signed-out sign-in link and signed-in account entry are described in `components/global-shell.yaml`; the Konase panel interaction contract is in `components/konase-panel.yaml`. The shared compact utility toolbar is described in `components/action-icon-bar.yaml`; the EntryBrowser result-table contract is in `components/entry-browser.yaml`, and the editable Form-scoped table contract is in `components/form-table.yaml`. The invitation Join surface is described in `components/invitation-join.yaml`; page files live under `pages/`. The persistent shell owns visible section context on Space-scoped routes: pages must not restate the active section identity. Pages own nested object or task identity. The mobile bottom bar changes the navigation mechanism without changing context ownership. The unauthenticated root route offers a single Login action; `/login` owns authentication methods, authentication state, and recovery. Space selection follows authentication. Product overview lives in Docs, the single documentation authority; `/about` redirects there. The Space-scoped shell uses a persistent Home / Forms / Search / Settings sidebar on desktop and the same destinations in a mobile bottom bar. Route behavior, API calls, and loading/error states remain authoritative in the corresponding TSX files and tests.

Implemented page routes include Space home/dashboard and saved Composition list/revision views, Form-first New Entry, Entries and history/restore, the list-only Forms list with form-scoped Entry lists and column types, keyword/advanced search, saved SQL and saved queries, the Form-owned Asset reference workspace, persistent Settings sections, and connection testing.

When changing a route:

1. update the TSX route and its tests;
2. update the matching page YAML route/status/components;
3. keep links between page IDs valid;
4. run the frontend/docsite checks through the root `mise` tasks.
