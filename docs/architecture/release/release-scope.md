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
- read-only MCP entry-list resource;
- portable API protocol shared by CLI and browser/WASM;
- Rust/Deno tests and CI gates.

## Limited or unavailable

- remote CLI asset upload is available in backend/API mode through the REST
  asset operation;
- index run/stats are local core-mode commands;
- general audit-log listing APIs are not exposed; authorization events remain
  portable Space state;
- browser-local persistence, offline-first editing, and sync are not
  implemented;
- this tree has a local release-validation task but no publishing workflow.

Release documentation and changelogs must use these boundaries rather than
planned capability.
