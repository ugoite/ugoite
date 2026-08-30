---
title: "Konase control-plane boundary"
sidebar:
  order: 4
---

Konase is a portable client-side Work runtime layered above Ugoite's shared
application behavior. It lets humans and agents work with user-owned
Knowledge without becoming its owner. The first delivery unit is intentionally
UI- and transport-free.

## Current implementation

crates/ugoite-konase owns:

- Work and bounded Job state;
- structured Observations and deterministically byte-bounded Context Capsules;
- serializable Events and Effects;
- the replaceable AgentRuntime contract.

`ugoite-konase-rig` implements that contract with Rig's sans-IO `AgentRun`.
It creates a fresh run per Job, pauses at model and MCP boundaries, and drops
the run at completion. Rig types and conversation state remain inside the
adapter; neither is persisted in Konase state or exposed through WASM.

The step function is deterministic. It never starts an async runtime and
never performs network, filesystem, storage, or model-provider I/O. A host
executes StartJob, CallModel, CallMcp, AskConfirmation, and Emit effects and
sends the result back as an Event.

ContextBuilder bounds the serialized Context Capsule as a whole, in addition
to its per-field limits. Capability metadata is admitted as an atomic
`{name, description, input_schema, effect}` payload under its own aggregate
budget, so normalization never leaves a model-visible capability without its
usable schema. `effect` is provider-neutral read/write metadata; it may be
absent when the host cannot establish the capability's effect. When a
UserSubmitted event creates StartJob, the builder receives the remaining byte
budget of the complete StepResult, keeping the portable effect boundary
independent of host/provider payload limits.

ugoite-wasm exposes the same semantics through konase.version, konase.new,
konase.step, and konase.context. The WASM adapter does not perform network
I/O; browser JavaScript remains responsible for fetch and other host effects.
Capability metadata includes a bounded, provider-neutral JSON input schema. Host
adapters preserve the MCP tool contract through this boundary; synthetic host
capabilities such as `resources/read` provide an explicit schema as well.

## Durability boundary

Konase state, agent memory, raw model context, pending effects, and execution
observations are disposable Work. The Context Builder uses bounded recent
observations and explicitly selected resource contents; it does not define an
append-only transcript contract. A Work result is not durable merely because a
model produced it.

When a user or host decides that a result should persist, it is promoted through
Ugoite's normal Knowledge mutation path. MCP save/delete and Work-scoped undo
therefore use the existing Space and Change/Run/Undo semantics. Konase does not
own a second transcript, database, authorization policy, or recovery path.

The control plane records the observed Knowledge outcome independently from the
model's Job outcome. A successful host result for a capability annotated as a
write produces `saved`; a failed result produces `write_failed`; a completed
Work without an observed write remains `unchanged`. The model's final text and
the fact that it requested a tool are not persistence evidence. The CLI and
browser expose this outcome separately and only make Work-scoped undo available
after a successful write result.

Konase may eventually help propose a reusable View or task-specific tool. The
definition, if saved, is ordinary Space-owned Knowledge; a separate adapter
renders it, and rendered/runtime state remains disposable Experience. This is a
future capability, not a shipped application builder.

## Host adapter status

The native CLI now provides the first host path: it connects to the
authenticated Ugoite MCP endpoint with the official rmcp client and uses one
configured model provider. It exposes `ugoite.search`, lazy
`resources/read`, `ugoite.save`, and Work-scoped `ugoite.undo`; the Host binds
each Work's writes to one Ugoite Run ID through MCP request metadata and maps
MCP `readOnlyHint` annotations into the provider-neutral capability effect. It
creates a fresh Rig run for each Job and keeps provider and transport types
inside the CLI/adapter boundary.

The browser Host and Konase UI now provide the same one-Job path. The panel
starts a browser-approved MCP device credential for the current Space, checks
the returned Space UID, and resolves the MCP endpoint from protected-resource
metadata. The credential, model key, and browser signing key stay in page
memory only. Space navigation invalidates the panel lifetime token, so late
Work results, errors, progress, and undo completions are discarded by the
browser adapter instead of mutating the newly rendered Space. Cancellation is
not required for this boundary. The Host executes model/MCP effects and uses
the existing `ugoite/runId` metadata for Work-scoped writes and undo. It maps
MCP `readOnlyHint` annotations and reports the same observed Knowledge outcome
as the CLI. It does not persist chat history or browser-local Space data. Agent Plugins, native MCP
abstractions, and other provider frameworks remain outside this MVP. They must
implement the contracts above without leaking provider/framework types into
the Konase or Ugoite public/domain contracts.
