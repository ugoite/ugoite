---
title: 'Policy Traceability'
---

Ugoite's governance stack is organized as:

1. **Philosophy** answers _why_ the product is built this way.
2. **Policies** answer _how_ those ideas become engineering constraints.
3. **Requirements, specifications, and features** answer _what_ concrete
   behavior must exist in the product.

The Foundation and Policy domains migrated into Mitase are authoritative in
`docs/mitase`. This directory remains the read-only legacy policy snapshot and
its downstream links are preserved as typed deferred edges in
`docs/mitase-migration/policy-edges.yaml` until the referenced domains are
migrated.

Each legacy entry in `policies.yaml` is intended to be readable from those three
angles:

- `linked_philosophies` shows which foundational beliefs the policy realizes.
- `description` explains the operational rule or constraint in more detail.
- `linked_requirements` shows which requirement categories are governed.
- `linked_specifications` points to the canonical design references for that
  policy.

The registries preserve traceability from philosophies to policies, requirements,
and implementation references. `docs/spec/features/features.yaml` and the
requirement categories declared in `linked_requirements` must stay aligned; CI
checks source and test references rather than relying on a second rendered model.

When updating the governance taxonomy:

- update philosophy and policy links together so the relationship stays
  bidirectional
- keep policy descriptions specific enough to explain why the rule exists
- preserve traceability from philosophy to policy to requirement/specification
  so drift remains visible in CI
- update the canonical Mitase policy graph and its migration manifest for
  migrated policy domains; do not edit the legacy snapshot
