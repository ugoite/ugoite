# SQL structure GUI retirement record

The former SQL structure GUI was retired under the text-first Composition
architecture. Issue [#4565](https://github.com/ugoite/ugoite/issues/4565) was
closed as not planned; restoring a structure editor requires a new product
decision. The removed toolbar, tree editor, filter/sort workspace, column
editor, and their UI-specific tests are historical Git content, not current
product requirements or CI obligations.

The checked `shared/sql/ugoite-sql-rules.json` retains the former SQL editor
inventory as a historical record. Its GUI operation inventory does not define
current Composition authoring and is not used to claim GUI coverage.

The Rust SQL Document parser, guarded edit operations, versioned WASM actions,
and typed protocol remain in the supported public surface. They serve parser,
normalization, source-range, identity, and published protocol contracts without
a production UI dependency. `check:wasm:sql-structure-fixtures` remains required
to protect those public fixtures; removing those actions requires a separately
planned breaking release. Current SQL authoring uses text, Rust lint, and
Composition planning/check APIs.
