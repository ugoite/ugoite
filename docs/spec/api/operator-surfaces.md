# Operator Surface Positioning

Ugoite exposes multiple operator surfaces on purpose. They share the same core
logic, but they serve different primary operators, trust models, and workflow
needs. This guide explains when to choose MCP, CLI, or REST and how that choice
traces back to the governance model.

## Governance Trace

- **PHIL-004** requires AI freedom to stay inside explicit trusted constraints.
- **POL-015** defines MCP as the governed, AI-facing operator surface.
- **POL-010** keeps AI autonomy inside trusted tools, validated interfaces, and
  audit paths.
- **POL-005** keeps the CLI as the explicit human/operator automation surface.
- **POL-004** keeps REST as the application/backend contract surface instead of
  a duplicate business-logic layer.
- **REQ-API-012**, **REQ-API-013**, and **REQ-API-014** keep the MCP surface
  safe, current-state, and clearly positioned in the docs.

## Surface Positioning

| Surface | Primary operator | Choose it when | Current posture |
| --- | --- | --- | --- |
| **MCP** | AI agents and assistants | You need protocol-level AI access to Ugoite resources inside explicit trust boundaries | `v0.1` ships a resource-first baseline: one read-only `ugoite://{space_id}/entries/list` resource, no prompts, no tools |
| **CLI** | Humans and scripts | You need explicit, inspectable automation, direct local workflows, or scriptable operator tasks | First-class operator surface for visible automation |
| **REST API** | Frontend and typed application clients | You need stable request/response contracts for product behavior and browser/app integration | Thin adapter surface for UI and service-facing contracts |

## Choosing Between MCP, CLI, and REST

- Use **MCP** when an AI client should read Ugoite resources through a governed
  protocol surface. Today that means the current resource-first baseline, not
  prompt execution or tool-style workflows.
- Use **CLI** when a human operator or script needs explicit commands,
  stdout/stderr visibility, and direct local inspection.
- Use **REST** when frontend or service clients need stable HTTP
  request/response contracts for product behavior.
- Keep durable business rules in `ugoite-core`; backend and CLI adapt that
  shared logic to their own protocols instead of re-implementing it.

## Current `v0.1` Boundary

In `v0.1`, MCP is intentionally narrower than CLI or REST. Choose CLI or REST
today for workflows that need writes, richer orchestration, or broader
application behavior. Treat broader MCP resources, prompts, and tools as
planned `v0.2` expansion rather than current surface area.
