# CLI guide

The `ugoite` binary supports local core mode and remote backend/API mode.

## Core mode

```bash
ugoite config set --mode core
ugoite space list /path/to/workspace
ugoite space create /path/to/workspace/spaces/demo
ugoite entry list /path/to/workspace/spaces/demo
ugoite query /path/to/workspace/spaces/demo --sql "SELECT id, title FROM entries LIMIT 10"
ugoite index run /path/to/workspace/spaces/demo
```

## Backend mode

```bash
ugoite config set --mode backend --backend-url http://127.0.0.1:8000
eval "$(ugoite auth login --mock-oauth)"
ugoite space list
ugoite entry list demo
```

Use a full `/path/to/root/spaces/<id>` path in core mode and a bare Space ID in backend/API mode. `ugoite config current` prints the routing mode. When core mode rejects a value, the error echoes the rejected input and points you back to the filesystem path form.

Space, Entry, Form, search, saved SQL, SQL-session, and most asset operations use the corresponding adapter. `index run` and `index stats` are core-only. Remote CLI asset upload, service-account CRUD, and audit-log commands are unavailable in this release.

For CLI-only iteration, `mise run test:cli` runs the CLI package tests before the full workspace gate.
