---
title: "Current release scope"
sidebar:
  order: 2
---

This page records the capability boundary of the current release so packaging,
support, and product claims stay aligned with the implementation.

## Included

- local CLI core mode over operator-owned Spaces;
- Rust REST server with authentication, Space membership/roles, entries, forms,
  assets, preferences, search, saved SQL, and SQL query sessions;
- server-backed browser application;
- single non-root container image and Helm chart;
- MCP v1 search/save/delete facade with lazy Entry, history, schema, and Form
  resources;
- portable API protocol shared by CLI and browser/WASM;
- Rust/Deno tests and CI gates.

## Limited or unavailable

- remote CLI asset upload uses the same portable multipart operation as the API
  client, REST, and frontend surfaces;
- index run/stats are local core-mode commands. `index run` rebuilds the
  internal AssetText DerivedRelation, while stats reports derived health;
- general audit-log listing APIs are not exposed; authorization events remain
  portable Space state;
- browser-local persistence, offline-first editing, and sync are not
  implemented;
- the `Release Publish` workflow now publishes versioned release artifacts and
  verifies the published Compose and CLI quick starts; an operator-supported
  release still requires a successful published version.

Release documentation and changelogs must use these boundaries rather than
planned capability.
