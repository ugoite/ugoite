---
title: "Ugoite specification index"
description: Executable specifications, requirements, interfaces, and implementation references for Ugoite.
sidebar:
  order: 1
---

**Updated:** 2026-06-29\
**Implementation status:** Rust/Deno v0.1 stream in progress

Ugoite is a private, portable Knowledge Space for humans and AI. Its foundation
is expressed as three boundaries: **Knowledge persists**, **Work may
disappear**, and **Knowledge can become tools**. User-owned Space directories
are authoritative; deployment and storage operators may host them without
becoming Knowledge authority. Indexes, query sessions, and runtime state are
derived or disposable.

## Current boundary

- Local CLI core mode directly opens Spaces.
- The Rust server exposes REST, the small MCP v1 semantic facade, and static browser
  hosting.
- The browser is server-backed.
- v0.1 supports mandatory user authentication, owner bootstrap, Passkey/WebAuthn
  login, opaque browser sessions, owner-approved Space access recovery,
  recovery-code + recovery-TOTP Account Self-Recovery,
  Remote CLI device authentication, Space membership/ACL enforcement,
  authenticated MCP access, and authorized audit reads.
- OIDC authentication, invitation-gated account creation, external identity
  linking/unlinking, and Federated browser sessions are also supported within
  the v0.1 security boundary.
- Browser-local persistence and optional sync are planned.
- administrator recovery, account discovery, agent/service-account principals,
  generic OAuth client compatibility, audit CRUD, and remote CLI
  asset upload remain future or limited capability. TOTP is a recovery-only
  factor and is not a normal login method.
- View/Application Definitions, renderers, low-code composition, and
  Knowledge-to-tools runtime behavior are future scope. v0.1 freezes their
  authority boundary but does not ship an application builder.

## Read the specification map

The specification is organized by the question it answers. The migrated slice
uses the schema-native Mitase graph under `docs/mitase`; unmigrated domains keep
their existing source files and machine-readable registries. This page makes
the reading order and the single semantic authority boundary explicit.

Behavior changes are specified alongside their implementation and verification
evidence. When evidence is incomplete, the gap remains explicit rather than
rewriting the requirement to fit the available proof.

## Migrated domain authority

Foundation, Policy, Search, Entry, Form, API, and the Asset lifecycle are represented in the canonical
Mitase records at `docs/mitase` for the current dogfood slice. These records
are the semantic source of truth for the migrated domains; their corresponding
legacy Foundation, Policy, Requirement, and Feature YAML are migration evidence
only and cannot override the canonical representation. The legacy Asset
requirement and feature registries remain read-only migration snapshots and
cannot override `docs/mitase/requirements/assets.yaml` or
`docs/mitase/features/assets.yaml`. The API-specific legacy
requirement and per-area feature YAML are no longer part of Mitase's declared
inventory; they remain read-only migration snapshots until the broader
`docs/spec` cleanup is complete. Changed-ownership enforcement remains staged
until it can be scoped safely to the migrated slice.
Other requirement and feature domains remain authoritative in their existing
`docs/spec` records until migrated.

`docs/mitase` is an intentional Ugoite specification surface for the Mitase
schema, not a second product authority. As legacy registry machinery and
unmigrated domains are retired, their corresponding `docs/spec` records may be
removed after the equivalent canonical records, evidence, and scoped ownership
rules have been reviewed.

- **Core model:** [data model](data-model/overview.md),
  [features](features/index.md), and the canonical machine-readable Foundation
  record at `docs/mitase/philosophies/foundation.yaml`.
- **Interfaces:** [REST API](api/rest.md),
  [OpenAPI](https://github.com/ugoite/ugoite/blob/main/docs/spec/api/openapi.yaml),
  [MCP](api/mcp.md), [operator surfaces](api/operator-surfaces.md), and
  [UI specifications](ui/index.md).
- **Requirements and stories:** [requirements](requirements/index.md) and
  [user stories](stories/index.md).
- **Architecture contracts:** [architecture overview](architecture/overview.md),
  decisions, stack, future-proofing, and the Space catalog.
- **Operations and quality:** [policy traceability](policies/index.md),
  [security](security/overview.md), [testing and CI](testing/strategy.md),
  [quality](quality/error-handling.md),
  [product metrics](product/success-metrics.md), and
  [versions](versions/index.md).

Use the [operator guides](../guide/index.md) for procedures. This section
describes the repository specification boundary; the migrated dogfood
representation is authored in `docs/mitase`, while unmigrated domains remain
authored in `docs/spec` until they are migrated.

## Module matrix

| Module              | Responsibility                                                       |
| ------------------- | -------------------------------------------------------------------- |
| `ugoite-domain`     | portable domain types and validation                                 |
| `ugoite-api-client` | transport-neutral HTTP operation protocol                            |
| `ugoite-storage`    | OpenDAL-backed storage mechanics                                     |
| `ugoite-iceberg`    | Catalog-backed Form tables, batch append, query, and publication Pins |
| `ugoite-core`       | application service and persistence behavior                         |
| `ugoite-konase`     | client-side Work/Job control semantics and serializable host effects  |
| `ugoite-server`     | REST/MCP/auth/static-hosting adapter                                 |
| `ugoite-cli`        | local and remote command adapter                                     |
| `ugoite-wasm`       | JSON/C ABI over portable Rust crates                                 |
| `frontend`          | SolidStart UI and JavaScript fetch adapter                           |
| `docsite`           | Starlight build shell that renders the repository-level `docs/` tree |

## Sources of truth

- REST implementation and generated contract: `crates/ugoite-server` and
  `/openapi.json`.
- Portable remote-operation contract: `crates/ugoite-api-client`.
- Application behavior: `crates/ugoite-core`.
- Filesystem/object-storage behavior: `crates/ugoite-storage` plus core modules.
- Browser behavior: `frontend` (currently server-backed).
- Task and CI surface: root `mise.toml`, `deno.json`, and
  `.github/workflows/ci.yml`.

Machine-readable registries under `features/`, `requirements/`, `ui/`, and
`docs/version/` must reference existing source and test paths. Planned
capability must be labeled planned rather than represented as implemented.
