---
title: "Konase control-plane boundary"
sidebar:
  order: 4
---

Konase is a client-side control plane layered above Ugoite's portable
application behavior. The first delivery unit is intentionally UI- and
transport-free.

## Current implementation

crates/ugoite-konase owns:

- Work and bounded Job state;
- structured Observations and bounded Context Capsules;
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

ugoite-wasm exposes the same semantics through konase.version, konase.new,
konase.step, and konase.context. The WASM adapter does not perform network
I/O; browser JavaScript remains responsible for fetch and other host effects.

## Durability boundary

Konase state, raw model context, pending effects, and execution observations
are disposable client state. The Context Builder uses bounded recent
observations and explicitly selected resource contents; it does not define an
append-only transcript contract. Meaningful Knowledge outcomes continue to use
Ugoite's existing Space and Change/Run/Undo semantics.

## Host adapter status

The native CLI now provides the first host path: it connects to the
authenticated Ugoite MCP endpoint with the official rmcp client and uses one
configured model provider. It exposes `ugoite.search`, lazy
`resources/read`, `ugoite.save`, and Work-scoped `ugoite.undo`; the Host binds
each Work's writes to one Ugoite Run ID through MCP request metadata. It
creates a fresh Rig run for each Job and keeps provider and transport types
inside the CLI/adapter boundary.

The browser Host and Konase UI are subsequent delivery slices. Agent Plugins,
native MCP transport abstractions, and other provider frameworks remain
outside this MVP. They must implement the contracts above without leaking
provider/framework types into the Konase or Ugoite public/domain contracts.
