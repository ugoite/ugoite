---
title: "Develop Ugoite"
description: Run the repository from source and complete the local authentication path.
sidebar:
  label: "Overview"
  order: 1
---

Development starts at the repository root and uses the same Rust server,
frontend, docsite, and validation tasks as CI. The browser remains server-backed
in the current release.

## Seed local sample data

Use the root task to create a portable sample Space for local development:

```bash
mise run seed
```

The default root is `./data`; the generated Space directory uses its immutable
UUIDv7 and the `dev-seed` slug. It uses the `renewable-ops` scenario and
approximately 50 entries. Pass arguments after `--` or set the `UGOITE_SEED_*`
environment variables documented by `scripts/dev-seed.sh` when a different root,
Space ID, scenario, entry count, or deterministic seed is needed:

```bash
mise run seed -- --space-id demo --scenario lab-qa --entry-count 10 --seed 7
```

The helper refuses to overwrite an existing target Space. Choose another Space
ID or remove the local development data intentionally before seeding again.

## Development path

1. Follow [Local development login](local-dev-auth-login.md) to start from
   source and complete the first-run authentication flow.
2. Use the repository root `mise` tasks for formatting, checks, tests, and the
   docsite build.
3. When changing the browser or API boundary, read the matching
   [architecture](../../architecture/index.md) and
   [executable specification](../../spec/index.md).

## Place the Rust build cache on another disk

Cargo stores intermediate Rust build artifacts in the configured `build-dir`.
The repository default follows Cargo's cache home, but Cargo's
`CARGO_BUILD_BUILD_DIR` environment variable takes precedence when a machine
needs the cache on another volume. Keep that choice in the developer's shell
profile (for example, `~/.zprofile` on macOS):

```sh
export CARGO_BUILD_BUILD_DIR="/path/to/large-disk/ugoite/cargo-build"
```

`CARGO_TARGET_DIR` controls final build outputs separately and remains the
repository's shared `target/rust` location.

## Optional SSH workflow

SSH is optional and is useful for development tools that require a standard
OpenSSH connection to the container. See [SSH access to the development
container](devcontainer-ssh.md) for setup and connection instructions.

## Run container-backed E2E in the devcontainer

The devcontainer includes the maintained Docker-in-Docker Feature. It starts a
separate Docker daemon inside the development container, so the Compose-backed
E2E runner can build and start the `ugoite:e2e` image without using the host
Docker daemon.

After rebuilding or reopening the devcontainer, verify the inner daemon and
run the representative E2E flow:

```bash
docker info
docker compose version
mise run e2e:smoke
```

The devcontainer is configured as privileged because Docker-in-Docker requires
it. The host must provide Docker and the devcontainer should use the same CPU
architecture as the host. E2E images and containers are owned by the inner
daemon and are independent of the host's Docker image list.
