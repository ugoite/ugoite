# Published v0.1.1 named Saved SQL fixture

This is a frozen Space produced with the published `v0.1.1` Linux x86_64 CLI,
not synthesized by current code. The GitHub Release archive
`ugoite-v0.1.1-x86_64-unknown-linux-gnu.tar.gz` SHA-256 was verified against
its published checksum:
`7d1a102b0e12df268334f9a1e1530142300b40af754177f0ba168ec34d4d05f5`.
The extracted executable SHA-256 is
`186aa6ecc4987f566a2c7e8bea8fc0b6f7827b6fb865fc584e1310af8cb00493` and
release tag `v0.1.1` resolves to commit
`3c26c8e8ad36fc02dec63fca543218b9bf0f05d8`.

The released executable created a Space, created `Audit Named Query` as
`SELECT $label AS answer` with a declared string variable `label`, and updated
it using its first revision as parent to `SELECT $label AS answer, 2 AS source`.
`expected.json` records both revisions and current SQL/name/variables. The SQL publication and revision bytes were copied byte-for-byte from that
Space. The fixture uses a descriptive metadata key identifier and retains
the synthetic signing material used by the released executable.
`SHA256SUMS` covers every file beneath `spaces/`.

The released SQL Form schema has `sql` and `variables` but no `name` field.
The named display label is persisted in the shared Entry title, exposed by the
read-only revision projection as `extra_attributes.ugoite_entry_title`. A
current-source CLI built from CI run `44350646058` (`source_sha`
`058bb63455048be07e780a97ece585efe2b2d161`; Linux x86_64 CLI archive SHA-256
`be81838050c0bfae923288c283926e2ff7358c656c30aa7d5ff3dd9c2b21c561`; extracted
executable SHA-256
`7d78826f473d45f8b0b31f1c4680aa796e1cc7bc92061fa054fb12b1caebbbb2`) reproduced
the original boundary: `sql saved get` returned `name: null` and `sql saved run`
returned `Saved SQL integrity mismatch` for both revisions. The focused Rust
fixture test checks read/list names, exact-revision execution/history, and
byte-preserving reads.

The v0.1.0 CLI release does not expose `sql saved-create` or
`sql saved-update`; this fixture records the earliest named-SQL persisted bytes
among those two published CLI releases, v0.1.1. It does not claim compatibility
for every v0.1.0 API-created object or every historical Space.
