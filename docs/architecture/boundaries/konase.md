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
adapter; neither is persisted in Konase state or exposed through WASM. When a
model turn contains multiple tool calls, the adapter validates and queues the
whole batch, then emits its MCP effects one at a time in model order. It feeds
all of that batch's results back to Rig together before advancing to the next
model turn.

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

ugoite-wasm exposes the same semantics through konase.version (protocol v2), konase.new,
konase.step, and konase.context. The WASM adapter does not perform network
I/O; browser JavaScript remains responsible for fetch and other host effects.
Capability metadata includes a bounded, provider-neutral JSON input schema. Host
adapters preserve the MCP tool contract through this boundary; synthetic host
capabilities such as `resources/read` provide an explicit schema as well. The
protocol version advances when this portable state schema changes; pre-v1
internal Konase state is rejected rather than migrated.

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
inside the CLI/adapter boundary. Each model request is bounded by the
`UGOITE_MODEL_TIMEOUT_SECS` setting (120 seconds by default). Timeout,
transport, and provider failures are converted into the existing
`KonaseEvent::HostFailed` event with the matching model request ID, so the
Work/Job becomes failed and its pending effect is cleared without discarding
an already observed Knowledge write or its undo availability.

Before a model-requested `ugoite.save` or `ugoite.undo`, the Rig adapter pauses
with a one-shot confirmation bound to that exact MCP request. It retains the
original arguments and emits the same request only after approval; denial
discards queued tool calls and ends the Job. The CLI asks for explicit `y` or
`yes` on a terminal and denies when terminal input is unavailable. A save is
reported as `saved` only after the MCP mutation receipt passes validation.
Approving a write does not change MCP credentials, Space scope, ACL, or server
validation.

The browser Host and Konase UI provide the same one-Job path. The panel starts
a browser-approved MCP device credential for the current Space, checks the
returned Space UID, and resolves the MCP endpoint from protected-resource
metadata. The credential, model key, and browser signing key stay in page
memory only.

Before a model-requested `ugoite.save` or `ugoite.undo`, the browser Host retains
the original MCP request and presents a bounded preview derived from its
arguments. Field values are summarized by type and length; sensitive values are
hidden. Approval applies once to that Work, request ID, Space, operation, and
argument snapshot. The Host checks that snapshot immediately before MCP
dispatch. Missing callbacks, denial, expiry, Space navigation, panel unmount,
or Host disposal fail closed. A mutation already sent to MCP cannot be recalled,
but its response is not applied to a newly rendered Space.

The browser Host treats known save and undo operations as writes even when MCP
omits their read-only annotations. Search is read-only only when tools/list
declares it read-only; unsupported or unknown effects fail closed. It uses the
existing `ugoite/runId` metadata for Work-scoped writes and undo, and reports
`saved` or enables Work Undo only after validating the canonical MCP mutation
receipt. A lost or malformed receipt is not retried or reported as a confirmed
save. The user-operated Undo button remains a direct, explicit Work-scoped
Undo. Space navigation and unmount invalidate pending confirmation and discard
late Work results, errors, progress, and undo completions. The browser does not
persist chat history or Space data. Agent Plugins, native MCP abstractions, and
other provider frameworks remain outside this MVP. They must implement the
contracts above without leaking provider/framework types into the Konase or
Ugoite public/domain contracts.
