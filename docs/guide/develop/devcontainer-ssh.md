---
title: "SSH access to the development container"
description: Connect to the Ugoite development container with OpenSSH.
sidebar:
  order: 3
---

## Overview

SSH is an optional development workflow for tools that communicate with an
environment through standard OpenSSH. Normal Ugoite development does not
require SSH setup.

## Security model

The workflow uses public-key authentication only and logs in as `vscode`.
Direct root login, password authentication, SSH agent forwarding, and remote
TCP forwarding are disabled. Local TCP forwarding is supported for development
services.

The SSH server listens on `127.0.0.1:2222` inside the container. Ugoite does
not publish or forward an SSH port in the Dev Container configuration. The
dedicated private key stays on the host under
`~/.ssh/ugoite-devcontainer/`; setup transfers only its public key to the
container. SSH host keys are verified with the dedicated known_hosts file.
This is a convenience for local development, not a hardened production SSH
environment.

## Setup

Run setup from a host terminal at the Ugoite repository root, not from a
terminal inside the development container:

```bash
mise run devcontainer:ssh
```

The equivalent command is:

```bash
./scripts/devcontainer-ssh setup
```

Setup requires the Dev Container CLI on the host and is idempotent. It creates
the dedicated host key, installs the container policy and public key, refreshes
host-key trust, and verifies a real SSH connection.

## Configure OpenSSH

Setup does not modify developer-owned SSH configuration automatically. To make
the profile available to normal `ssh` commands, optionally add this line to
`~/.ssh/config`:

```sshconfig
Include ~/.ssh/ugoite-devcontainer/config
```

## Connect

With the optional Include:

```bash
ssh ugoite-devcontainer
```

Without modifying `~/.ssh/config`:

```bash
ssh \
  -F ~/.ssh/ugoite-devcontainer/config \
  ugoite-devcontainer
```

The login user is `vscode` and the workspace is `/workspace`.

## Local forwarding

Expose a development service listening on port 3000 inside the container:

```bash
ssh \
  -L 3000:127.0.0.1:3000 \
  ugoite-devcontainer
```

## After rebuilding

After recreating the development container, run:

```bash
mise run devcontainer:ssh
```

This refreshes the Ugoite SSH policy, installed public key, host-key trust,
generated proxy configuration, and generated OpenSSH configuration.

## Troubleshooting

Use standard OpenSSH diagnostics:

```bash
ssh \
  -vvv \
  -F ~/.ssh/ugoite-devcontainer/config \
  ugoite-devcontainer
```
