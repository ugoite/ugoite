---
title: CLI guide
sidebar:
  order: 2
---

`ugoite` has two endpoint modes. Core mode opens operator-owned Space
directories directly and does not perform human login. Backend/API mode uses a
Space ID and the configured remote endpoint.

## Choose an endpoint

Use `config current` to inspect the active mode and `config set` to change it:

```bash
# Local-first core mode
ugoite config set --mode core
ugoite config current
ugoite space list /path/to/workspace

# Server-backed mode
ugoite config set --mode backend --backend-url https://ugoite.example.com
ugoite config current
```

In core mode, commands that address a Space take its full local path, such as
`/path/to/workspace/spaces/demo`. In backend/API mode, pass the bare Space ID,
such as `demo`. `ugoite space list` takes the workspace root only in core mode;
omit the positional argument in backend/API mode.

## Remote CLI authentication

`ugoite auth login` generates a fresh P-256 key and starts browser-approved
device authorization. Open the printed verification URL on a signed-in browser,
approve the exact Space and actions, and return to the CLI. The default actions
are `read,create,update`; delete and share are not implicit. REST CLI tokens are
opaque, short-lived, issuer-audience credentials and every REST request carries
DPoP. The CLI stores the private key in the OS keychain when available and falls
back to an owner-only file (0600 on packaged Linux).

```bash
ugoite config set --mode core
ugoite space list /path/to/workspace
```

`ugoite auth logout` is local logout: it deletes the local credentials and
private key. Server-side device-grant revocation is a separate operation. MCP
credentials use `{issuer}/mcp` as resource and audience; REST and MCP
credentials cannot cross-use. Use the explicit MCP target when pairing the
Konase host; the CLI discovers the server's protected-resource metadata, so the
raw resource URL is not needed:

```bash
ugoite auth login --for mcp
ugoite konase --prompt "Find the latest project note"
```

Each model request has a finite timeout. The default is 120 seconds; set
`UGOITE_MODEL_TIMEOUT_SECS` to a positive number of seconds to adjust it. A
timeout, transport failure, or provider failure marks the current Work as
failed and reports the observed Knowledge outcome separately. Knowledge saved
before the failure remains saved, and its Work-scoped undo remains available.

In an interactive `ugoite konase` session, press Ctrl-C while the model is
waiting to interrupt the local model wait. The current Work is reported as
failed, the prompt returns, and a later prompt can start a new Work. If a save
completed before the interruption, its Knowledge outcome remains saved and the
Work-scoped undo remains available. Ctrl-C while idle, during MCP/undo work, or
while connecting retains the CLI's process-exit behavior. With `--prompt`, an
interrupted model wait reports the interruption and exits non-zero.

This is host-local interruption: dropping the model request future stops the
CLI from waiting, but does not guarantee that a remote provider has stopped
generation or billing. The CLI does not cancel MCP writes, because the server
may have committed a write even if its response has not reached the client.

`ugoite auth login` and `ugoite auth login --for mcp` create different
credential targets. Run the matching login command if a saved credential is for
the other target. Remote CLI asset upload remains future scope.

## Spaces and entries

Create and inspect a Space, using the path or ID appropriate for the selected
mode:

```bash
ugoite space create /path/to/workspace/spaces/team-notes   # core
ugoite space create team-notes                              # backend/API
ugoite space get /path/to/workspace/spaces/team-notes
ugoite entry list /path/to/workspace/spaces/team-notes
```

An entry ID is a user-chosen storage-safe slug. It may contain ASCII letters,
digits, `-`, and `_`, must be 1–128 bytes, and must not contain path separators,
control characters, or `.`/`..` path segments. `first-note` is therefore an
example ID, not a reserved name.

The smallest complete first-entry workflow is:

```bash
ugoite entry create /path/to/workspace/spaces/team-notes first-note \
  --content $'---\nform: Entry\n---\n# First note\n\n## Body\n\nHello from Ugoite.'
ugoite entry get /path/to/workspace/spaces/team-notes first-note
ugoite entry list /path/to/workspace/spaces/team-notes
```

After creating a `Meeting` Form as described below, add Form-backed metadata by
including its name in the frontmatter:

```bash
ugoite entry create /path/to/workspace/spaces/team-notes meeting-2026-07-17 \
  --content $'---\nform: Meeting\n---\n# Planning\n\n## Notes\n\nAgenda'
```

Use `entry update`, `entry history`, `entry revision`, and `entry restore` for
revisions. Updates can include `--parent-revision-id` to enforce optimistic
conflict checks. `entry delete` appends a deletion tombstone to the revision
history. The currently accepted `--hard-delete` flag also writes a tombstone;
permanent removal is not available in this release.

## Forms

Forms define the fields and defaults available when creating entries. List
existing Forms, inspect supported field types, and write a definition from a
JSON file:

```bash
ugoite form list /path/to/workspace/spaces/team-notes
ugoite form list-types
ugoite form get /path/to/workspace/spaces/team-notes Meeting
ugoite form update /path/to/workspace/spaces/team-notes meeting.json
```

Run `ugoite form <command> --help` for the exact JSON shape and flags for the
installed version.

## Search and SQL

Keyword search and saved SQL work in both endpoint modes:

```bash
ugoite search keyword /path/to/workspace/spaces/team-notes planning
ugoite sql lint 'SELECT _ugoite_id, _ugoite_title FROM note LIMIT 10'
ugoite sql saved-list /path/to/workspace/spaces/team-notes
ugoite sql saved-get /path/to/workspace/spaces/team-notes recent-notes
```

Use `query` for an ad-hoc read-only DataFusion SQL session over Iceberg Forms:

```bash
ugoite query /path/to/workspace/spaces/team-notes \
  --sql 'SELECT _ugoite_id, _ugoite_title FROM note LIMIT 10'
```

Each Form is queryable through its lowercase name. Relations expose the Form
fields plus `_ugoite_id`, `_ugoite_title`, `_ugoite_created_at`, and
`_ugoite_updated_at`; `entries`, `links`, and `assets` are not SQL relations;
references and assets are values in typed Form columns.

## Indexes and assets

Index maintenance is local-core functionality in this release:

```bash
ugoite index stats /path/to/workspace/spaces/team-notes
ugoite index run /path/to/workspace/spaces/team-notes
ugoite index run /path/to/workspace/spaces/team-notes --component asset-text
```

The CLI reports that these commands are unavailable in backend/API mode. Asset
upload and delete use the local Space path in core mode and the bare Space ID
in backend/API mode:

```bash
# Core mode
ugoite asset upload /path/to/workspace/spaces/team-notes ./diagram.png
ugoite asset delete /path/to/workspace/spaces/team-notes asset-id

# Backend/API mode: upload through the API client or REST surface; the remote
# CLI upload command is intentionally unavailable in this release.
ugoite asset delete team-notes asset-id
```

The API client and frontend still send the REST `file` multipart part with
`application/octet-stream` media type. Use `--filename` in core mode to choose
the logical filename; the server applies the same safe-basename rules.

Every command has exhaustive, version-specific help. Use
`ugoite <command> --help` or `ugoite <command> <subcommand> --help` before
copying flags into automation.
