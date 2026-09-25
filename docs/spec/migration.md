---
title: "Migration status"
description: Which specification domains are canonical in Mitase and which remain in docs/spec.
---

This page records the migration ledger. It preserves source meaning: missing or
ambiguous evidence becomes an explicit gap and never narrows a Requirement.
Mitase validates declared relationships and evidence; it does not execute tests
or become a second Knowledge authority.

All canonical documents under `docs/mitase` now use Mitase's
`mitase/authoring/v2` full-document form. The migration changes only the
authoring schema declaration; the complete requirements, features, policies,
philosophies, bindings, claims, and verification metadata remain intact and
continue to normalize into the same semantic graph. The repository-level
`mitase.yaml` intentionally remains `mitase/config/v1`, which is the current
Mitase configuration schema and is separate from specification authoring.

## Migrated domain authority

Foundation, Policy, Search, Entry, Form, Indexer, API, Asset, Frontend, E2E,
Integrity, and the Storage Space foundation, authenticated creation contract,
plus connector/access/routing/preference slice are represented in the canonical
Mitase records at `docs/mitase` for the current canonical slice. These records are
the semantic source of truth for the migrated domains; their corresponding
legacy Foundation, Policy, Requirement, and Feature YAML are migration evidence
only and cannot override the canonical representation. The API-specific legacy
requirement registry is retired and is no longer part of Mitase's declared
inventory. The canonical API graph is the only semantic authority for that
domain. Frontend legacy requirement YAML is likewise retired and no longer part
of Mitase's declared inventory. The Asset and E2E requirement YAML have been
retired entirely. The legacy Indexer requirement registry is likewise retired;
its canonical Search and Form graphs are the only semantic authority for derived
indexing, structured query, word-count, and validation behavior. The legacy
Search requirement registry is also retired; the canonical Search graph at
`docs/mitase/requirements/search.yaml` is the only semantic authority for
keyword search, structured query, frontend search behavior, and derived relation
maintenance. The legacy Entry requirement registry is also retired; the
canonical Entry graph at `docs/mitase/requirements/entries.yaml` is the only
semantic authority for Entry creation, revision, mutation, history, Markdown
extraction, and interface behavior. The legacy Form requirement registry is also
retired; the canonical Form graph at `docs/mitase/requirements/forms.yaml` is
the only semantic authority for Form schema governance, CRUD operations,
reserved metadata, row references, attribution, and typed property conversion.
The legacy Frontend requirement registry is also retired; the canonical Frontend
graph at `docs/mitase/requirements/frontend.yaml` is the only semantic authority
for routes, components, interaction surfaces, API clients, and exact Frontend
verification evidence. The legacy Integrity requirement registry is likewise
retired and no longer part of Mitase's declared inventory. The migrated Storage
Space foundation records are likewise no longer semantic authority in their
legacy registry. The duplicate-create conflict contract is now canonical;
remaining Storage records continue to be migrated in focused slices.
Changed-ownership enforcement remains staged until it can be scoped safely to
the migrated slice. The canonical Operations graph now represents `REQ-OPS-001`
through `REQ-OPS-024`, together with `REQ-OPS-043` and `REQ-OPS-044`; later
Operations records remain migration evidence until their focused Mitase slices
are reviewed. Other requirement and feature domains remain authoritative in
their existing `docs/spec` records until migrated. The retired Asset requirement
registry is not retained as a second semantic source; its canonical replacement
is the Mitase Asset graph described above.

`docs/mitase` is an intentional Ugoite specification surface for the Mitase
schema, not a second product authority. As legacy registry machinery and
unmigrated domains are retired, their corresponding `docs/spec` records may be
removed after the equivalent canonical records, evidence, and scoped ownership
rules have been reviewed.

## Cross-surface preflight authority map

For the v0.3 preflight, authority is assigned by the behavior slice rather than
by a whole directory name:

| Domain | Canonical semantic source | Remaining or supporting source | Status for the preflight |
| --- | --- | --- | --- |
| Search and EntryQuery | `docs/mitase/requirements/search.yaml` and `docs/mitase/features/search.yaml` | Retired legacy Search registry is not authoritative. | Canonical for the current structured query and count criteria. Browser cancellation and request-generation behavior remain implementation follow-ups under #3120. |
| API, MCP resource safety, and Konase Host outcomes | `docs/mitase/requirements/api.yaml` and `docs/mitase/features/api.yaml` | `crates/ugoite-server` and `/openapi.json` define the REST implementation and contract; `crates/ugoite-server/src/mcp.rs` owns the MCP resource projection. | Canonical for API outcomes, untrusted-resource framing, explicitly selected Context admission, and Host write approval/receipt criteria. The portable operation registry remains `ugoite-api-client`; the preflight does not restate SQL semantics. |
| Forms | `docs/mitase/requirements/forms.yaml` and `docs/mitase/features/forms.yaml` | `docs/version/v0.2/product-ux.yaml` is a release UX tracker, not a behavior authority. | Canonical for Form schema and operations. The read-only MCP Form resource is a separate surface observation, not an upsert path. |
| Frontend behavior | `docs/mitase/requirements/frontend.yaml` and `docs/mitase/features/frontend.yaml` | `docs/mitase/requirements/ux.yaml` and `docs/mitase/features/ux.yaml` own their explicit interaction criteria. `docs/spec/ui/ux-route-inventory.md` is an inventory aid. | Canonical only for migrated requirements and explicit UX criteria; tracker completion remains unverified where its acceptance evidence is absent. |
| Journey and mutation outcomes | `docs/mitase/requirements/journey.yaml`, `docs/mitase/features/journey.yaml`, plus API and Entry records for their respective slices. | Runtime behavior is implemented in shared Rust/core and adapter surfaces. | Canonical for linked journey outcomes. Receipts, ACL, conflict, append-only history and Restore must be traced to their own claims and tests; source presence alone is not proof. |
| Storage | `docs/mitase/requirements/storage.yaml` and `docs/mitase/features/storage.yaml` for the migrated Space, creation, connector/access/routing/preference slices. | `docs/spec/requirements/storage.yaml` remains semantic authority for unmigrated Storage requirements. | Partial migration; do not treat all Storage behavior as migrated. |
| Operations | `docs/mitase/requirements/ops.yaml` and `docs/mitase/features/ops.yaml` for `REQ-OPS-001`–`REQ-OPS-024`, `REQ-OPS-043`, and `REQ-OPS-044`. | `docs/spec/requirements/ops.yaml` remains semantic authority for later, unmigrated Operations records. | Partial migration; later records remain legacy-authoritative until reviewed and migrated. |
| Konase selected Context and Host confirmation | `REQ-API-012#criterion.selected-context-admission` and `REQ-API-017` in `docs/mitase/requirements/api.yaml`; `FEAT-API-001#binding.konase-host` in `docs/mitase/features/api.yaml`. | `docs/architecture/boundaries/konase.md` and the generated `v0.3-preflight` report locate current Host and verification artifacts. | CLI and Browser selected-resource read/preview/send boundaries and one-shot write approval/receipt behavior have exact verification claims. Broader mutation recovery and authorization-revocation compositions remain #3157. |

The selected-Context and write-approval Criteria fill previously explicit
Konase Host gaps in the canonical Mitase graph. The v0.2 release tracker remains
a separate UX status source, not a second semantic authority.

## Migration changed-scope rule (PR11 #2422)

Source of truth: `AGENTS.md` Specification contract. Mitase validates declared
specification relationships and evidence; it does not execute Ugoite tests, own
repository delivery, or become a second Knowledge authority.

- There is no Ugoite-specific exemption in the Mitase validator for
  migrated-criterion changed-scope validation, and none may be added.
  Baseline-aware changed-scope diagnostics for a migrated criterion are
  satisfied the same way as any other criterion.
- A migration PR satisfies the changed-scope rule by changing at least one
  canonically-owned artifact of that criterion. When the semantic definition has
  migrated, the canonical spec artifact (under `docs/mitase`) is in the change
  scope.
- Never fake-change a retired legacy implementation or test surface as
  "migration". Unchanged implementation/test artifacts remain acceptable exactly
  when a canonical spec artifact changed; changing only legacy-owned surfaces
  does not satisfy the rule.
- This policy does not weaken the repository-wide `mitase check .` readiness
  gate (`validation.preset: strict`, `validation.readiness.target: traceable`).
  The `changed` baseline stays `parent` with no exemption keys.
- Mitase stays pinned to an immutable `0.2.x` release via
  `tools/mitase.lock.toml` and `scripts/mitase`. Migration
  work never pins Mitase HEAD or a mutable branch.
