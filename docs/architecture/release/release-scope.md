---
title: "Current release scope"
sidebar:
  order: 2
---

This page defines what "current" and "stable" mean for Ugoite documentation so
packaging, support, and product claims stay aligned with the implementation.

- "Current" describes this documentation build and the implementation it was
  written against. It is never a release claim.
- "Stable" describes only a published release. A published release is identified
  by its immutable release tag and manifest, and described by its versioned
  release note under `docs/version/releases/`. Drafts and journey indexes never
  define stable behavior.

The boundary below is the one this documentation build describes. Stable claims
resolve to the versioned release note, not to this page.

Current main includes the Text-first Composition authoring and runtime described
in the [Composition contract](../contracts/composition.md), targeting unreleased
v0.2.2. Composition was not shipped in v0.2.1; its
[published release note](../../version/releases/v0.2.1.md) remains the stable
source. The View/Application renderer exclusion below concerns general
Knowledge-to-tools application rendering, not Text-first Composition.

## Included

- local CLI core mode over operator-owned Spaces;
- Rust REST server with entries, forms, assets, preferences, search, saved SQL,
  and stateless read-only SQL query;
- Form-first, server-backed browser application with structured Entry authoring
  and reads;
- canonical EntryQuery search and filtering across Browser, local CLI, and
  remote API;
- CLI Context selection and durable mutation Receipt output;
- authenticated remote CLI Asset upload and deletion;
- single non-root container image and Helm chart;
- MCP v1 search/save/delete facade with lazy Entry, history, schema, and Form
  resources;
- portable API protocol shared by CLI and browser/WASM;
- Rust/Deno tests and CI gates.

## Limited or unavailable

- Shared S3-compatible Space writes are supported only for endpoints that pass
  the exact configured-backend CAS probe and the independent-process acceptance
  lane. Other shared OpenDAL backends remain read-only. Local filesystem writes
  use the existing single-process and OS file-serialization path. Authorization
  state and protected content share one Catalog Head publication boundary;
- index run/stats are local core-mode commands. `index run` rebuilds the
  internal AssetText DerivedRelation, while stats reports derived health;
- read-only, authorization-checked Node and Space audit-event listing is
  exposed; audit-event mutation/CRUD remains unavailable;
- browser-local persistence, offline-first editing, and sync are not
  implemented; the current browser still requires the Rust server;
- portable View/Application Definitions, general Knowledge-to-tools renderers,
  and general application-builder behavior are not implemented. v0.1 freezes
  their authority boundary but does not ship a View DSL, general-purpose
  application builder, arbitrary code runtime, app-specific database, or
  parallel authorization authority;
- Passkey/WebAuthn login, opaque browser sessions, Space membership/ACL
  enforcement, owner-approved Space access recovery, recovery-code +
  recovery-TOTP Account Self-Recovery, Remote CLI device authentication,
  invitation-gated OIDC authentication/account linking, authenticated MCP
  access, and authorized audit reads are supported v0.1 capabilities;
- administrator recovery, managed service accounts, and audit CRUD are not
  supported capabilities. TOTP is recovery-only and is not a normal login
  method;
- the operator-dispatched candidate and promotion workflows publish only a
  candidate verified by the release smoke policy. Promotion is buildless;
  pre-publish verification records a separate receipt, while the published
  distribution check verifies exact assets, registry identity and availability,
  container health, and CLI version. An operator-supported release still
  requires a successful published version.
- release platform support is tiered: the four current CLI targets and the Linux
  `amd64`/`arm64` container image are Tier 1; Tier 2 is a best-effort or
  nightly-only future set and is currently empty; all other targets are
  unsupported until explicitly assigned a tier.

Release documentation and changelogs must use these boundaries rather than
planned capability.
