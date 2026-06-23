# ugoite-domain

Storage- and transport-neutral Ugoite domain types and validation.

This crate is intentionally usable by native and WebAssembly targets. It must not depend on server frameworks, async runtimes, storage operators, or the application service. Higher layers use it to keep identifiers, roles, forms, entries, and protocol-facing rules consistent.

```bash
cargo test -p ugoite-domain --locked
cargo check -p ugoite-domain --target wasm32-unknown-unknown --locked
```
