---
title: "Architecture North Star"
sidebar:
  order: 2
---

This document defines the product promise, authority model, present boundary,
target state, and invariants that guide Ugoite development.

## Product promise

Ugoite is a private, portable Knowledge Space for humans and AI. Knowledge is
owned by the operator, remains recoverable from operator-controlled
infrastructure, and can be used by different clients without being copied into a
new system of record.

> Knowledge persists. Work may disappear. Knowledge can become tools.

The third statement is a direction for the product, not a claim that v0.1 ships
a general application builder. Ugoite should make it possible to shape
Space-owned Knowledge into purpose-built Views and task-specific applications
while keeping ownership and authority in the Space.

## Ownership model

A Space directory or object-store prefix is the durable Knowledge boundary. It
contains authoritative Forms, Entries, Assets, saved SQL, Changes, portable
history, and the data required to recover them. Search indexes, SQL sessions,
and other acceleration structures are derived and replaceable.

A server can authenticate, authorize, and serve a Space, but it does not own a
hidden catalog, relational database, or recovery index for that Space. A browser
session, AI provider, agent runtime, or generated Experience is likewise not a
Knowledge owner.

Node accounts, sessions, credentials, and Node-to-Space bindings are separate
node-local control state below `_ugoite/nodes/{node-id}` by default, or in the
backend selected by `UGOITE_NODE_CONTROL_URI`. The encryption root supplied by
`UGOITE_NODE_SECRET_KEY` or `UGOITE_NODE_SECRET_FILE` is a separate recovery
input. A complete Node recovery set preserves the Space prefix, Node control
store prefix, and node secret independently.

## Knowledge, Work, and Experience

**Knowledge** is durable and inspectable. **Work** is the temporary state of
trying to understand or change Knowledge: model interaction, temporary context,
observations, intermediate reasoning, execution progress, retries, and tool
results. Konase is a portable Work runtime; its state and agent memory may be
discarded without changing Knowledge authority.

**Experience** is the layer that makes Knowledge useful for a purpose, such as a
table, dashboard, research view, structured data-entry screen, search interface,
or project workspace. Experience runtime state is replaceable. A future View or
Application Definition may become durable Space content, but its render cache,
open tab, transient result, and component state do not.

When Work or Experience produces a result worth keeping, a user or authorized
host promotes it through the normal Knowledge mutation path. Change, Run, and
Undo semantics remain the same regardless of whether the mutation came from a
human, CLI, browser, MCP client, or Konase-assisted workflow.

See [Knowledge, Work, and Experience](knowledge-work-experience.md) for the
conceptual boundary and its failure model.

## Authority and persistence

The Catalog Head is the only authoritative mutable catalog root. In the current
encoding it is `_ugoite/catalog/head.json`, published with an actual OpenDAL
ETag compare-and-swap. The v0.1 compatibility contract freezes this authority
semantics, not the physical filename or encoding. Immutable, checksum-protected
publication records under `_ugoite/catalog/publications/` link each successful
Head generation to the preceding one. Iceberg metadata, manifests, and data
files remain Iceberg-owned immutable objects.

Readers pin immutable Head and snapshot coordinates and never lock. Writers can
prepare immutable objects concurrently, but make a mutation visible only by
replacing Head with the exact ETag they read. Backends that cannot prove the
conditional read/create/replace contract are read-only unless an operator
explicitly selects single-process mode. Catalog authority and Form/Checkpoint
correctness do not depend on leases, heartbeats, TTLs, lock files, or fencing.
DerivedRelation cleanup records never establish visibility, authority, or
checkpoint state.

Knowledge mutations attach a `ChangeDescriptor` to immutable publications. The
publication command identity is the Change ID, so history is portable with the
Space prefix and can be reconstructed from the reachable chain. `RunId` is
correlation metadata only. Pins are exact Head-owned references to immutable
publications, and selective undo creates a new append-only publication rather
than rewinding Head or resurrecting a schema.

## Current state and control surfaces

- The Rust core reads and writes operator-owned Spaces.
- CLI core mode calls the core directly against a local workspace; backend/API
  mode uses the portable operation protocol.
- The Rust server exposes authenticated REST, the small semantic MCP facade, and
  static browser hosting.
- The browser is currently server-backed and has no complete local Space storage
  adapter. Browser-local persistence and optional synchronization are future
  runtime capabilities.
- WASM exposes portable domain/API protocol logic and does not own persistence,
  transport, or a model runtime.
- Konase exposes deterministic client-side Work/Job semantics and serializable
  host effects. It does not own Knowledge, provider state, or a durable agent
  database.

These are adapters and control surfaces around one application model. None may
introduce a parallel persistence or authorization authority.

## Target state

The browser should eventually open a local Space runtime and synchronize only
when the user enables an optional relay or collaboration service. Humans and
agents should be able to compose portable, inspectable Views and task-specific
tools from the same Space-owned Knowledge. Durable definitions, when introduced,
remain Space content; rendered and execution state remains runtime state.

The target does not require arbitrary JavaScript or Python execution, arbitrary
package installation, an app-specific backend or database, hidden durable
application state, or an application-specific authorization authority.

## Invariants

1. Operator-owned Space content is the portable source of truth; the Space
   prefix is the move and recovery unit.
2. Revisions and publication history are append-only; current state is derived
   and recovery never depends on a hidden database.
3. The Catalog authority (currently the Catalog Head) is the only authoritative
   mutable root. Object listing never establishes catalog state or publication
   order.
4. Forms own authoritative history. DerivedRelation Heads own only replaceable
   current builds for indexes and projections; they never become Space,
   checkpoint, or authorization authority.
5. Work runtime state, agent memory, and model-provider context do not become
   Knowledge authority.
6. Experience runtime state does not become Knowledge authority. Durable View or
   Application Definitions, when introduced, remain portable Space-owned
   content.
7. Generated tools do not require copying Knowledge into a second system of
   record or introducing a parallel authorization authority.
8. Humans and agents use the same underlying Knowledge semantics.
9. Domain and use-case behavior lives in reusable Rust crates; CLI, server,
   WASM, browser, and MCP transport code remain adapters.
10. Current and planned capabilities are documented separately.
