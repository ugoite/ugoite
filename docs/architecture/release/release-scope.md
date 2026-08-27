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
- S3-compatible and other non-local OpenDAL operators can serve authoritative
  Space mutations when startup or explicit revalidation proves the behavioral
  exact-read and conditional-write contract. A binding whose exact reads are
  unavailable fails as storage-unavailable; a readable binding whose conditional
  writes cannot be proved is `SharedReadOnly`. The health report exposes the
  selected mode and reason. Provider names do not grant or deny mutation
  admission;
- index run/stats are local core-mode commands. `index run` rebuilds the
  internal AssetText DerivedRelation, while stats reports derived health;
- read-only, authorization-checked Node and Space audit-event listing is
  exposed; audit-event mutation/CRUD remains unavailable;
- browser-local persistence, offline-first editing, and sync are not
  implemented;
- Passkey/WebAuthn login, opaque browser sessions, Space membership/ACL
  enforcement, owner-approved Space access recovery, recovery-code +
  recovery-TOTP Account Self-Recovery, Remote CLI device authentication,
  authenticated MCP access, and authorized audit reads are supported v0.1
  capabilities;
- OIDC linking, administrator recovery, managed service accounts, audit CRUD,
  and remote CLI asset upload are not supported v0.1 capabilities. TOTP is
  recovery-only and is not a normal login method;
- the `Release Publish` workflow now publishes versioned release artifacts and
  verifies the published Compose and CLI quick starts; an operator-supported
  release still requires a successful published version.

Release documentation and changelogs must use these boundaries rather than
planned capability.
