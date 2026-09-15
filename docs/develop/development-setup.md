---
title: "Development Setup"
description: Build Ugoite locally, run its server and frontend, and validate changes.
sidebar:
  order: 2
---

This is the contributor path for the Rust server, SolidStart frontend, and
Astro docsite. It uses the repository's root tasks and keeps local development
authentication separate from production credentials.

## Outcome

The repository builds from source, the local server/frontend/docsite loop starts,
and the contributor can complete the first-run Passkey flow against local data.

## Prepare the toolchain

From the repository root, install the pinned tools and dependencies:

```bash
mise run setup
```

This installs the Rust/WASM target, fetches locked Rust dependencies, installs
Deno workspace dependencies, and installs the Playwright browser used by E2E.

## Start locally

Use the combined development task:

```bash
mise run dev
```

It builds the debug WASM adapter, starts the Rust server, the frontend, and the
docsite. The helper creates a local node secret under `target/` and uses `data/`
as the default local root. Override `UGOITE_ROOT` or the development endpoint
variables when a different local layout is needed.

For sample content, run the seed helper separately:

```bash
mise run seed
```

The helper refuses to overwrite an existing target Space. Use
`bash scripts/dev-seed.sh --help` for the supported scenario and size options.

## Development authentication

Open the local server's one-use setup URL from the server output, register the
initial Passkey, save the recovery codes, and complete the second Passkey
ceremony. Then use the frontend at its local URL. The local CLI core mode can
use the same local Space without a server login; do not treat that as an
authentication bypass for server-backed behavior.

The local secret is disposable development state. Never reuse it as a deployed
node secret or commit it to the repository.

## Validate changes

Use the smallest relevant command while iterating, then run the root suite:

```bash
mise run fmt
mise run lint
mise run check
mise run test
```

For docsite-only changes, use `deno task --cwd docsite check` and
`deno task --cwd docsite build`. For a contributor change that affects packaged
outputs or E2E, use the corresponding `mise run ci:artifacts` or
`mise run e2e:smoke` lane after the focused checks.

When running browser E2E in the development container, confirm the isolated
Docker engine first:

```bash
docker info
mise run e2e:smoke
```

## Version boundary

The v0.1 line is published and maintained; the current product line is v0.1.1.
The active v0.2 direction is Product UX around completion, discoverability,
cross-surface consistency, validation clarity, recovery, and documentation
correctness. Knowledge-to-tools remains a North Star, not a shipped acceptance
claim. Do not treat browser-local persistence, a general application builder,
or arbitrary code execution as current contributor requirements.

## Optional development container access

SSH is optional. When a tool needs an OpenSSH connection to the development
container, run `mise run devcontainer:ssh` from the host repository root. The
workflow uses public-key authentication for the `vscode` user and local port
forwarding; it is not a production SSH procedure.

## Related

- [Cross-surface Features](cross-surface-features.md)
- [Repository Map](repository-map.md)
- [Engineering Principles](engineering-principles.md)
- [Documentation Development](documentation.md)
