---
title: "UI specifications"
---

Page YAML files describe the current Space-scoped browser routes. `page.title`
is route/document metadata and does not by itself require a visible heading.
`page.implementation: implemented` means a matching SolidStart route exists
under `frontend/src/routes`; it does not imply browser-local persistence. The
global account Security surface is specified in
`components/account-security.yaml`.

FormTable's Form-field columns, optional timestamps, and accessible column
picker are described in `components/form-table.yaml`.

The current browser is server-backed and authenticated. Shared navigation and
the Browser Konase panel reference are described in
`components/space-shell.yaml`; GlobalShell's signed-out sign-in link and
signed-in account entry are described in `components/global-shell.yaml`; the
Konase panel interaction contract is in `components/konase-panel.yaml`. Login
actions, recovery, and transient authentication states are described in
`components/login.yaml`; owner-approved Space recovery at `/recover` and its
one-time Recovery Code handoff are described in
`components/space-access-recovery.yaml`; the `/recover/account` factor form and
one-time Recovery Code handoff are described in
`components/account-recovery.yaml`;
first-run administrator setup and its recovery handoff are described in
`components/setup.yaml`. The `/device` consent context and hidden identifier
boundaries are described in `components/device-approval.yaml`. Account security
credential tabs and identifier disclosure are described in
`components/account-security.yaml`; the `/step-up` CLI approval task and its
opaque challenge boundary are described in `components/step-up.yaml`. The
shared compact utility toolbar is
described in `components/action-icon-bar.yaml`; the EntryBrowser result-table
contract is in `components/entry-browser.yaml`, and the editable Form-scoped
table contract is in `components/form-table.yaml`. The invitation Join surface
is described in `components/invitation-join.yaml`; page files live under
`pages/`. The persistent shell owns visible section context on Space-scoped
routes: pages must not restate the active section identity. Pages own nested
object or task identity. The mobile bottom bar changes the navigation mechanism
without changing context ownership. The unauthenticated root route offers a
single Login action; `/login` owns authentication methods, authentication state,
and recovery. Space selection follows authentication. Product overview lives in
Docs, the single documentation authority; `/about` redirects there. The
Space-scoped shell groups Assets, Forms, and Saved tools under Knowledge on
desktop, with Saved tools in the mobile More menu. Route behavior, API calls,
loading/error states remain authoritative in the corresponding TSX files and
tests.

Implemented page routes include Space home/dashboard and saved Composition
list/revision views, Form-first New Entry, Entries and history/restore, the
list-only Forms list with form-scoped Entry lists and column types,
keyword/advanced search, saved SQL and saved queries, the Form-owned Asset
reference workspace, persistent Settings sections, and connection testing.

When changing a route:

1. update the TSX route and its tests;
2. update the matching page YAML route/status/components;
3. keep links between page IDs valid;
4. run the frontend/docsite checks through the root `mise` tasks.
