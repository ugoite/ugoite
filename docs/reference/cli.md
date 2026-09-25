---
title: "CLI"
description: Command families, connections, contexts, authentication, and output conventions.
sidebar:
  order: 2
---

`ugoite <command> --help` is the authority for the installed CLI's flags,
arguments, output fields, and exit behavior. This page explains how to choose a
mode and find the right command; task steps live in [Use
Ugoite](../use/index.md).

## Connections and contexts

Named connections describe how the CLI reaches Knowledge. A `core` connection
opens an operator-owned Space directory directly; it is the local, server-free
path and does not require human login. A `backend` or `api` connection points
at a remote endpoint and uses the server's authentication and authorization.

Create connections with the canonical TOML model:

```bash
ugoite config init
ugoite config connection add local --type core --root /path/to/workspace
ugoite config connection add remote --type api --url https://example.com/api
```

Named contexts (`ugoite context --help`) select one connection plus one Space
by its immutable Space UID, with an optional named credential profile. Most
Space-bound commands use the selected context, so no Space path or UID is
passed on every invocation. `--context <NAME>` overrides once without changing
the selection, and `ugoite config current` inspects the resolved connection,
Space UID, and credential name (never secrets).

Start a local workspace with `ugoite space create demo` after selecting a
connection. Creation registers the new Space as the current context
automatically (`--no-context` opts out). CLI configuration is disposable
work-environment state in `./.ugoite/config.toml` (project-local, preferred
when present), `~/.ugoite/config.toml`, and `~/.ugoite/credentials.json`;
deleting it never deletes Knowledge.

For a `core` connection, a context's immutable Space UID resolves to one local
directory. New Spaces use `<root>/spaces/<SPACE_UID>`; existing Space 0.1
directories are located by their read-only metadata identity without being
renamed. The shared Space compatibility classifier decides compatibility.
`ugoite config current` prints the resolved connection, Space UID, and
credential name (never secrets) in a stable section order: `Config sources:`,
`Write target:`, `Current context:`, `Connection:`, `Root:` (local) or
`Endpoint:` (remote), `Space:`, `Credential:`.

## Authentication

`ugoite auth login` starts browser-approved device authorization for backend
use. It creates a fresh P-256 key and DPoP-bound REST credentials. `ugoite auth
logout` removes the local credential and key; run `ugoite auth --help` for the
exact credential lifecycle and device options.

MCP credentials are a separate target. Use `ugoite auth login --for mcp` when
pairing a Konase/MCP host; REST credentials cannot be used for MCP, and MCP
credentials cannot be used for REST. The matching task and command help are the
authority for the currently supported pairing flow.

Named credential profiles (`ugoite auth login --connection work --credential
alice-work`) store per-connection credentials in `~/.ugoite/credentials.json`
for contexts to reference by name; `ugoite auth profile --credential
alice-work` shows metadata without secrets, and `ugoite auth logout
--credential alice-work` removes one profile while leaving the others intact.
Secrets never enter `config.toml`.

Agent principals and service-account automation are future design material, not
current v0.1 client capabilities. Do not treat them as an alternative login
path in a production procedure.

## Output conventions

Interactive terminals use compact human output. Piped success output and
`--format json` (also exposed as `-o json` where supported) use the command's
machine-readable JSON value. Piped failures use the JSON error envelope on
stderr and preserve the command's exit-code mapping. Use `--format table` or
`--format plain` only where the command's help advertises those projections.

ANSI emphasis is limited to human-facing TTY output. Pipes, JSON, `NO_COLOR`,
and `TERM=dumb` remain plain and stable. Mutation receipts expose the resource
identifier, revision, Change, and Run metadata when the operation commits;
these durable IDs are the values to record or pass to a later recovery task.

The CLI does not promise that human styling is a machine contract. Scripts
should consume the documented JSON shape and stderr/exit behavior, and should
obtain the exact field names from the installed command's `--help` output.

## Command families

The command families map to Knowledge tasks rather than separate Browser and
CLI documentation trees:

- `space`: create, inspect, and list Spaces;
- `form`: list, inspect, and save Form definitions;
- `entry`: create, read, update, list, history, restore, and delete Entries;
- `asset`: upload and delete Space-owned file content in core and authenticated
  remote modes;
- `sql`: saved-query and read-only SQL workflows (lint, query, count, and
  saved subcommands);
- `change` and `run`: inspect the Space timeline and append inverse Changes for
  recovery;
- `index`: rebuild derived local indexes; and
- `auth`, `config`, and `konase`: configure endpoint credentials and run the
  supported model-assisted experience.

Start with the task page for the outcome, then use the matching command help:

## Common Knowledge commands

The primary authoring and recovery commands are:

```bash
ugoite form save <FILE>
ugoite entry create --form <FORM> [--field KEY=VALUE ...]
ugoite entry create --id <ENTRY_ID> --form <FORM> ...  # advanced import/reconciliation
ugoite entry update <ENTRY_ID> --field KEY=VALUE ...
ugoite entry history <ENTRY_ID>
ugoite entry restore <ENTRY_ID> <REVISION_ID>
ugoite sql saved list
ugoite sql saved get <SQL_ID>
ugoite sql saved create --sql <SQL_OR_FILE> [--name <NAME>]
ugoite sql saved update <SQL_ID> [--name <NAME> | --untitled] [--sql <SQL_OR_FILE>]
ugoite sql saved delete <SQL_ID>
ugoite sql lint <SQL>
ugoite sql query <SQL_OR_FILE>
ugoite sql count <SQL_OR_FILE>
ugoite sql export <SQL_OR_FILE> --max-rows <N> [--page-size <N>] [--output <PATH>]
ugoite change list
ugoite change revert <CHANGE_ID> [--message <MESSAGE>]
ugoite run undo <RUN_ID>
ugoite pin create <NAME>
ugoite pin list
ugoite pin read <NAME>
ugoite pin diff --from <NAME> --to <NAME>
ugoite pin delete <NAME>
```

Entry creation returns the generated Entry ID in its mutation receipt. Use that
returned ID for later reads, updates, history, and restore commands. The
optional `--id` is an advanced override for imports and reconciliation. Saved
SQL commands are grouped under `sql saved`; names are optional, and a blank
name represents an untitled query. An omitted Saved SQL update revision uses
the current revision read by the CLI; `--parent-revision-id` remains available
when an automation needs an explicit concurrency precondition. Pins capture
local operator snapshots and are managed with the `pin` command family.

| Outcome | Task page | Command authority |
| --- | --- | --- |
| Open or create a Space | [Spaces](../use/spaces.mdx) | `ugoite space --help` |
| Define a Form | [Create a Form](../use/forms.mdx) | `ugoite form --help` |
| Create or edit an Entry | [Create and Edit Entries](../use/entries.mdx) | `ugoite entry --help` |
| Add an Asset | [Add an Asset](../use/assets.mdx) | `ugoite asset --help` |
| Search and structured filter | [Search and Filter](../use/search.mdx) | `ugoite entry list --help` |
| Saved SQL and expert query | [Saved SQL and expert query](../use/sql.mdx) | `ugoite sql --help` |
| View history | [View History](../use/history.mdx) | `ugoite entry history --help`, `ugoite change --help` |
| Restore or undo | [Restore and Undo](../use/restore.mdx) | `ugoite entry restore --help`, `ugoite change --help`, `ugoite run --help` |
| Ask about selected Knowledge | [Konase](../use/konase.mdx) | `ugoite konase --help` |

Use the command's subcommand help for exact required arguments, mode
availability, JSON fields, and exit behavior. The reference intentionally does
not duplicate every flag from the executable.

## Authorities

- CLI behavior is authoritative in the current binary (Clap):
  `ugoite <command> --help` wins over prose.
- REST behavior is authoritative in `crates/ugoite-server` and the
  server-generated `/openapi.json`.
- Requirements and evidence are authoritative in Mitase declarations; docs
  explain and never create a second protocol registry.

## Related reference

- [REST API and OpenAPI](rest.md) for server endpoints and schemas.
- [MCP](mcp.md) for the authenticated semantic facade.
- [Configuration](configuration.md) for operator environment variables.
