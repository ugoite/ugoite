# Ugoite specification index

**Updated:** 2026-06-23  
**Implementation status:** Rust/Deno v0.1 stream in progress

Ugoite is a local-first knowledge-space system based on three principles: **Low Cost**, **Easy**, and **Freedom**. Operator-owned Space directories are authoritative; indexes and query sessions are derived.

## Current boundary

- Local CLI core mode directly opens Spaces.
- The Rust server exposes REST, one read-only MCP resource, and static browser hosting.
- The browser is server-backed.
- Browser-local persistence and optional sync are planned.
- Passkey/TOTP login, managed service-account CRUD, and audit-log APIs are not operational in this release.

## Navigation

- [Architecture](architecture/overview.md)
- [REST API](api/rest.md) and [OpenAPI](api/openapi.yaml)
- [MCP](api/mcp.md) and [operator surfaces](api/operator-surfaces.md)
- [Data model](data-model/overview.md)
- [Feature registry](features/README.md)
- [Requirements](requirements/README.md)
- [UI specifications](ui/README.md)
- [Security](security/overview.md)
- [Testing and CI](testing/strategy.md)
- [Versions](versions/index.md)
- [Operator guides](../guide/concepts.md)

## Module matrix

| Module | Responsibility |
|---|---|
| `ugoite-domain` | portable domain types and validation |
| `ugoite-api-client` | transport-neutral HTTP operation protocol |
| `ugoite-storage` | OpenDAL-backed storage mechanics |
| `ugoite-core` | application service and persistence behavior |
| `ugoite-server` | REST/MCP/auth/static-hosting adapter |
| `ugoite-cli` | local and remote command adapter |
| `ugoite-wasm` | JSON/C ABI over portable Rust crates |
| `frontend` | SolidStart UI and JavaScript fetch adapter |
| `docsite` | Astro documentation site |
