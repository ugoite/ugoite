# Policy Traceability

Ugoite's governance stack is organized as:

1. **Philosophy** answers _why_ the product is built this way.
2. **Policies** answer _how_ those ideas become engineering constraints.
3. **Requirements, specifications, and features** answer _what_ concrete
   behavior must exist in the product.

Each entry in `policies.yaml` is intended to be readable from those three
angles:

- `linked_philosophies` shows which foundational beliefs the policy realizes.
- `description` explains the operational rule or constraint in more detail.
- `linked_requirements` shows which requirement categories are governed.
- `linked_specifications` points to the canonical design references for that
  policy.

The rendered design pages in the docsite use those links to show how a
philosophy connects to specific policies. Feature-area badges on the policy and
relations pages are derived from the requirement categories declared in
`linked_requirements` and the manifest-backed feature inventory in
`docs/spec/features/features.yaml`, so those two sources must stay aligned.

When updating the governance taxonomy:

- update philosophy and policy links together so the relationship stays
  bidirectional
- keep policy descriptions specific enough to explain why the rule exists
- preserve traceability from philosophy to policy to requirement/specification
  so drift remains visible in CI
