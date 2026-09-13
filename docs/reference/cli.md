---
title: "CLI"
description: Command families, modes, and output conventions.
sidebar:
  order: 2
---

`ugoite <command> --help` is the authority for flags and JSON shapes. This page
explains how to think about the CLI; task steps live in
[Use Ugoite](../use/index.md).

## Modes

Core mode opens operator-owned Space directories directly and performs no human
login. Backend mode addresses a Space by its immutable Space UID with the
configured remote endpoint. A Space slug is mutable display metadata used when
creating or renaming a Space, not a remote identity. Inspect with
`ugoite config current` and change with `ugoite config set`.

## Authentication

`ugoite auth login` starts browser-approved device authorization with a fresh
P-256 key and DPoP-bound credentials. `ugoite auth logout` deletes local
credentials only. MCP credentials use a separate `--for mcp` target and cannot
cross-use REST credentials.

## Output conventions

TTY output stays concise and uses the Quiet Accent human presentation: muted
headers and labels, cyan primary identifiers, and semantic colors only for
success, warnings, and errors. Tables are borderless, use two-space column gaps,
and calculate widths before styling. `NO_COLOR`, `TERM=dumb`, pipes, and JSON
output never receive ANSI sequences.

Piped success output and `--format json` keep the existing JSON schemas.
Piped failures keep the existing JSON error envelope, stderr ownership, and
exit-code mapping. Human receipts and errors retain their canonical wording and
line structure; only TTY emphasis changes. Every command documents its exact
behavior in `--help` before copying flags into automation.

## Command families

Auth, Config, Space, Entry, Form, Asset, Search, SQL and Query, Index, and
Konase. Index maintenance and asset upload are local-core functionality in
this release; asset delete also works in backend mode. Run
`ugoite <command> --help` for the exact per-mode surface.

## Related

- [CLI guide](../guide/automate/cli.md) for the narrative walkthrough.
- [Spaces](../use/spaces.mdx) and [Create and Edit Entries](../use/entries.mdx)
  for task steps.
