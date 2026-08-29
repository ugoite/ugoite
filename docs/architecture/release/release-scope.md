---
title: "Current release scope"
sidebar:
  order: 2
---

This page records the capability boundary of the current release so packaging,
support, and product claims stay aligned with the implementation.

## Included

- local CLI core mode over operator-owned Spaces;
- Rust REST server with entries, forms, assets, preferences, search, saved SQL,
  and SQL query sessions;
- server-backed browser application;
- single non-root container image and Helm chart;
- MCP v1 search/save/delete facade with lazy Entry, history, schema, and Form
  resources;
- portable API protocol shared by CLI and browser/WASM;
- Rust/Deno tests and CI gates.

## Limited or unavailable

- remote CLI asset upload is intentionally unavailable in this release; the API
  client, REST, and frontend asset upload surfaces remain available;
- S3-compatible and other non-local OpenDAL operators are available for
  authoritative Space mutations only after a runtime probe verifies exact reads,
  create-if-absent, stale-read rejection, stale-write rejection, and a single
  concurrent Head-CAS winner. Unsupported, unavailable, or unverified operators
  remain read-only. Ordinary authorization mutations use exact
  `AuthorizationState` CAS; server timestamps are independent and are used only
  for maintenance age comparisons, without a distributed lease or fencing
  protocol;
- index run/stats are local core-mode commands. `index run` rebuilds the
  internal AssetText DerivedRelation, while stats reports derived health;
- read-only, authorization-checked Node and Space audit-event listing is
  exposed; audit-event mutation/CRUD remains unavailable;
- browser-local persistence, offline-first editing, and sync are not
  implemented;
- portable View/Application Definitions, Knowledge-to-tools renderers, and
  general application-builder behavior are not implemented. v0.1 freezes their
  authority boundary but does not ship a View DSL, low-code editor, arbitrary
  code runtime, app-specific database, or parallel authorization authority;
- Passkey/WebAuthn login, opaque browser sessions, Space membership/ACL
  enforcement, owner-approved Space access recovery, recovery-code +
  recovery-TOTP Account Self-Recovery, Remote CLI device authentication,
  invitation-gated OIDC authentication/account linking, authenticated MCP
  access, and authorized audit reads are supported v0.1 capabilities;
- administrator recovery, managed service accounts, audit CRUD, and remote CLI
  asset upload are not supported v0.1 capabilities. TOTP is recovery-only and is
  not a normal login method;
- the operator-dispatched `Release Publish` workflow publishes versioned release
  artifacts and verifies the published Compose and CLI quick starts; an
  operator-supported release still requires a successful published version.

Release documentation and changelogs must use these boundaries rather than
planned capability.
