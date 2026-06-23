# Feature registry

`features.yaml` indexes per-area YAML files. Each operation records the canonical HTTP method/path and existing backend, frontend, core, and optional CLI source locations.

The registry is generated from the current Rust/OpenAPI surface, not from roadmap intent. `implemented` means the route and referenced adapter exist. `contracted_unavailable` is used for `/auth/login`, whose route exists but intentionally returns `403` in this release.

Validate feature routes against [`../api/openapi.yaml`](../api/openapi.yaml) and referenced files against the repository. MCP is documented separately because it is not part of the portable application-operation manifest.
