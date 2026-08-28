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

The step function is deterministic. It never starts an async runtime and
never performs network, filesystem, storage, or model-provider I/O. A host
executes StartJob, CallMcp, AskConfirmation, and Emit effects and sends the
result back as an Event.

ugoite-wasm exposes the same semantics through konase.version, konase.new,
konase.step, and konase.context. The WASM adapter does not perform network
I/O; browser JavaScript remains responsible for fetch and other host effects.

## Durability boundary

Konase state, raw model context, pending effects, and execution observations
are disposable client state. The Context Builder uses bounded recent
observations and explicitly selected resource contents; it does not define an
append-only transcript contract. Meaningful Knowledge outcomes continue to use
Ugoite's existing Space and Change/Run/Undo semantics.

## Planned adapters

The Rig adapter, Agent Plugins loader, native MCP transport, CLI TUI, browser
Host, and Konase UI are subsequent delivery slices. They must implement the
contracts above without leaking provider/framework types into the Konase or
Ugoite public/domain contracts.
