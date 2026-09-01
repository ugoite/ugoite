---
title: Model Context Protocol v1
---

Ugoite MCP v1 is a small, authenticated semantic facade at `POST /mcp`.
It is stateless and uses MCP protocol revision `2026-07-28`; clients do not
send `initialize`, use `Mcp-Session-Id`, or select a Space by internal ID.

MCP v1 is a separately versioned semantic contract. Its stable tool/resource
surface is governed independently from the v0.1 Knowledge compatibility floor:
MCP evolution must not change Space ownership, Catalog authority, or
append-only history semantics, and a storage encoding change must not silently
change the meaning of an MCP v1 operation.

The stable surface is deliberately narrow:

- `ugoite.search` returns five sanitized summaries by default and at most 25
  when requested, with stable `ugoite://entry/{id}` resource links. Entry bodies are
  loaded only when a resource is read.
- `ugoite.save` creates an Entry without `id` or updates one opaque Entry with
  `id`. A new Entry may use plain Markdown: the MCP semantic facade
  canonicalizes it to the built-in `Entry` Form's `Body` field, using a leading
  H1 as the title. Complete Entry Markdown with `form` frontmatter remains
  supported for new Entries that select another Form. Updates must keep the
  existing Entry's Form frontmatter.
- Save validation failures are returned as semantic tool errors. For example,
  `INVALID_INPUT` identifies missing Entry/Form structure,
  `UNKNOWN_FORM_FIELDS` identifies unsupported sections, and
  `FORM_VALIDATION_FAILED` identifies missing or invalid Form fields. These
  errors include safe validation detail without exposing storage layout.
- `ugoite.undo` reverses all changes made by the current Konase Work through
  the existing Run undo semantic operation. Its empty model-facing argument
  object is unchanged; the Host supplies the Work Run ID in
  `_meta["ugoite/runId"]` for both `ugoite.save` and `ugoite.undo`.
- `ugoite.delete` performs only an authorized soft delete.

The capability boundary is explicit:

| Capability | v0.1 status | Contract |
| --- | --- | --- |
| Search Entries | Supported | `ugoite.search` returns bounded sanitized summaries; bodies are lazy resources. |
| Create/update Entry | Supported | `ugoite.save` uses the normal Entry/Form validation and authorization boundary. |
| Saved SQL | Not supported | Use the authenticated REST/CLI Saved SQL and SQL-session surfaces; no MCP SQL tool is advertised. |
| ACL administration | Not supported | MCP credentials are scoped to an already approved Space; membership and ACL changes remain REST owner operations. |
| Duplicate tool calls | No general guarantee | Each call is an independent semantic operation. A client must use the returned result or reconcile its target; the MCP adapter does not promise blind replay idempotency. |
| Service accounts, audit CRUD, browser-local persistence | Future/reference | These are not v0.1 MCP capabilities. |

Tools are filtered before `tools/list`: read-only credentials see search,
write credentials additionally see save and undo, and delete is shown only to
an approved human device credential with effective Delete authority. Tool
descriptions and schemas contain no storage-specific Form, revision, Iceberg,
bucket, path, or storage details; they describe only the semantic Entry/Form
behavior needed by the model.

Resources are lazy and use stable opaque URIs:

- `ugoite://entry/{id}` — semantic Entry projection;
- `ugoite://entry/{id}/history` — append-only event projection;
- `ugoite://entry/{id}/schema` — the associated Form schema;
- `ugoite://form/{id}` — a semantic Form schema.

`resources/list` is empty and `resources/templates/list` contains only those
four templates. Search and resource results label user-controlled material as
`_untrusted_content: true`, sanitize it, and warn clients never to treat it as
instructions. They never expose storage layout or revision internals.

## Authentication

The protected resource is exactly `{issuer}/mcp`. Protected-resource metadata
is published at `/.well-known/oauth-protected-resource`; authorization-server
metadata is published at `/.well-known/oauth-authorization-server`. Human MCP
credentials are short-lived opaque Bearer credentials bound to issuer, node,
resource, Space, actions, credential generation, and the human device. An MCP
client requests a credential with `POST /oauth/device/authorization` using
`resource: {issuer}/mcp`, the requested Space/actions, and its public key; the
signed-in owner approves the displayed request at `/device`; the client
exchanges the device code at `/oauth/token` with its client assertion and the
same resource. Existing DPoP credentials may use `Authorization: DPoP` plus
one RFC 9449 proof. Agent credentials cannot use the MCP delete tool.

Cookie sessions are rejected at `/mcp`. For v0.1, the supported boundary is the
authenticated MCP protocol, its resource-bound credential flow, and ACL
behavior. The `/device` approval page accepts MCP-scoped requests and REST CLI
requests with an omitted resource; those credentials remain separated by
audience and cannot cross-use. Agent credential flows remain future scope. The
MCP adapter never asks for a Space ID, bucket, database, or
filesystem path.

The REST implementation remains `crates/ugoite-server`; `/openapi.json` is the
REST API source of truth. MCP JSON-RPC is intentionally not represented as a
one-to-one REST endpoint or as a storage-specific OpenAPI path.

This surface follows the design rule: breadth is cheap, depth is lazy. New
Ugoite capabilities should first be expressed through search and semantic
resources; a new MCP tool is a versioned exception, not the default result of
adding an internal service operation.

Each authenticated `tools/call` credential is limited to 60 calls per rolling
minute. Excess calls receive HTTP 429 with a JSON-RPC rate-limit error and
`Retry-After`; read/list operations are not charged against this limit.
