---
title: 'Entry and Form migration ledger'
description: 'Evidence and explicit gaps for the second and third live Mitase migration slices.'
---

This page records the live Entry and Form migration from Ugoite's legacy
registries into the canonical Mitase graph. The source remains available under
`docs/spec/` until the remaining domains are migrated. The migrated records are
based on Ugoite main at `30b0e9def9d07cad7305b2d6ffad04c25d66eca2`.

## Entry migration

`REQ-ENTRY-001` through `REQ-ENTRY-006` retain their implemented status and
gain exact current Rust test selectors where the repository has a named test
that proves the criterion. `REQ-ENTRY-007` through `REQ-ENTRY-009` remain
migration gaps because this slice does not yet provide a criterion-specific
exact proof target for them. The legacy `verification: traced` field is not
treated as proof by itself.

The Entry feature binds the canonical Iceberg implementation symbols for
creation, update, deletion, history, and restore. The history claim uses the
core test surface that owns that semantic. The legacy Form attribution
requirement describes the full lifecycle, but this slice narrows its canonical
criterion to Entry creation and uses a dedicated exact creation test because
one lifecycle test cannot be associated with multiple current implementation
targets under the Mitase relation rules. Update, delete, and restore
attribution remain migration gaps. The server and CLI remain adapters until
their exact criteria are migrated.

## Form migration

`REQ-FORM-001`, `REQ-FORM-002`, and `REQ-FORM-004` through `REQ-FORM-007`, plus
`REQ-FORM-009`, are
connected to exact Iceberg implementation symbols and named integration tests.
`REQ-FORM-003` remains untraced because its legacy record has no exact
criterion-specific test selector in this slice. `REQ-FORM-008` remains planned
and is catalog-only; its legacy test reference is not promoted to a current
claim.

No migration step runs a test or mutates a Space. Mitase records exact
relationships and validates declarative runner metadata; execution remains the
responsibility of Ugoite's repository tooling and CI.
