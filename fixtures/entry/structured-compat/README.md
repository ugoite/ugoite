# Structured compatibility corpus (D0)

0.1.x canonical path:

- Markdown compatibility input -> shared Rust conversion -> Structured Entry Draft
- Structured payload -> Structured Entry Draft
- Draft -> shared validation/normalization -> existing 0.1 persistence

Space format/version, storage encoding, Iceberg schema, and revision/history
semantics are unchanged. `RowReference` stays a typed `FieldValue::String`;
`AssetReference` reuses the existing domain type.

Each fixture pins one semantic area and checks three things:

1. `legacy_markdown_to_draft(markdown)` reaches the expected structured state.
2. `normalize_and_validate_draft(form, draft)` reaches the expected typed values.
3. The structured draft and the legacy Markdown draft normalize to the same
   durable outcome (legacy create and structured create agree).

`09-existing-space-reopen.json` additionally pins open -> mutate -> reopen:
the stored 0.1 representation round-trips through the same draft boundary.

`10-structured-authoring-parity.json` is the Lane 1 release evidence:
one representative Form across every field family (plus required and
unknown cases) that the frontend (WASM), CLI core, and CLI remote surfaces
converge on for stored values, Form identity, revision parentage,
validation codes, and reopen stability.
