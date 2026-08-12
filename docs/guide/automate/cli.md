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
ugoite auth login --device-name workstation --actions read,create,update
```

In core mode, commands that address a Space take its full local path, such as
`/path/to/workspace/spaces/demo`. In backend/API mode, pass the bare Space ID,
such as `demo`. `ugoite space list` takes the workspace root only in core mode;
omit the positional argument in backend/API mode.

## Authentication

Remote login uses device authorization. Open the displayed verification URL on
any already signed-in browser, compare the code and requested actions, and
approve the device. The CLI stores the rotating credential in the OS keychain
when available, or in `~/.ugoite/cli-credentials.json` with owner-only
permissions:

```bash
ugoite auth login --device-name workstation --actions read,create,update
ugoite auth profile
ugoite auth logout
```

`auth profile` reports metadata only. `auth logout` removes the local device
credential. The browser credential page can revoke a device that is lost. Core
mode does not need `auth login`.

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
```

The CLI reports that these commands are unavailable in backend/API mode. Asset
upload and delete use the local Space path in core mode and the bare Space ID
in backend/API mode:

```bash
# Core mode
ugoite asset upload /path/to/workspace/spaces/team-notes ./diagram.png
ugoite asset delete /path/to/workspace/spaces/team-notes asset-id

# Backend/API mode
ugoite asset upload team-notes ./diagram.png
ugoite asset delete team-notes asset-id
```

Remote upload sends the file as the REST `file` multipart part with
`application/octet-stream` media type and prints the returned `AssetReference`
as JSON. Use `--filename` to choose the logical filename; the server applies
the same safe-basename rules as core mode.

Every command has exhaustive, version-specific help. Use
`ugoite <command> --help` or `ugoite <command> <subcommand> --help` before
copying flags into automation.
