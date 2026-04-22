# CLI Guide

This guide explains the recommended release-install path for `ugoite`, then
covers the Cargo-based workflow contributors still use inside the repository.

This is the lowest-setup-cost local-first path today. In `core` mode the CLI
works directly against the local spaces directory, so you can start without a
Docker stack, frontend process, or browser login flow.

## Install the released CLI (recommended)

### Verify-first archive install

If you want the most auditable published path, install from an exact release
archive and verify its checksum before you extract or run anything:

```bash
VERSION=0.1.0
TARGET=x86_64-unknown-linux-gnu
BASE_URL="https://github.com/ugoite/ugoite/releases/download/v${VERSION}"

curl -fsSLO "${BASE_URL}/ugoite-v${VERSION}-${TARGET}.tar.gz"
curl -fsSLO "${BASE_URL}/ugoite-v${VERSION}-${TARGET}.tar.gz.sha256"

# Linux
sha256sum -c "ugoite-v${VERSION}-${TARGET}.tar.gz.sha256"
# macOS
shasum -a 256 -c "ugoite-v${VERSION}-${TARGET}.tar.gz.sha256"

tar -xzf "ugoite-v${VERSION}-${TARGET}.tar.gz"
mkdir -p "$HOME/.local/bin"
install -m 0755 ugoite "$HOME/.local/bin/ugoite"
ugoite --help
```

Swap `TARGET` for one of the published release targets listed below. This path
keeps checksum verification ahead of extraction and ahead of any installer
script execution.

### Bootstrap helpers (secondary)

If you want fewer manual steps and you already trust the bootstrap helper
itself, use one of these shortcuts after considering the verify-first archive
path above.

Install the public `ugoite` npm bootstrap package:

```bash
npm install -g ugoite
ugoite-install
ugoite --help
```

Pin an exact package version when you want the matching published release:

```bash
npm install -g ugoite@0.1.0
ugoite-install
ugoite --help
```

The published package metadata lives in `packages/ugoite/package.json`, while
the repository root `package.json` stays private tooling for Husky/commitlint
and release automation. The shared helper script behind the npm bootstrap and
rendered release installers lives at `scripts/install-ugoite-cli.sh`.

If you still prefer the direct shell bootstrap, keep it as an explicit
trust-the-script shortcut rather than the default recommendation:

```bash
curl -fsSL https://raw.githubusercontent.com/ugoite/ugoite/main/scripts/install-ugoite-cli.sh | bash
ugoite --help
```

Pin an exact version when you want a specific release:

```bash
curl -fsSL https://raw.githubusercontent.com/ugoite/ugoite/main/scripts/install-ugoite-cli.sh | env UGOITE_VERSION=0.1.0 bash
ugoite --help
```

Install an exact release with a platform-specific one-liner:

```bash
# Linux x86_64
curl -fsSL https://github.com/ugoite/ugoite/releases/download/v0.1.0/ugoite-v0.1.0-x86_64-unknown-linux-gnu.install.sh | bash

# Linux arm64
curl -fsSL https://github.com/ugoite/ugoite/releases/download/v0.1.0/ugoite-v0.1.0-aarch64-unknown-linux-gnu.install.sh | bash

# macOS x86_64
curl -fsSL https://github.com/ugoite/ugoite/releases/download/v0.1.0/ugoite-v0.1.0-x86_64-apple-darwin.install.sh | bash

# macOS arm64
curl -fsSL https://github.com/ugoite/ugoite/releases/download/v0.1.0/ugoite-v0.1.0-aarch64-apple-darwin.install.sh | bash
```

The installer writes `ugoite` into `~/.local/bin` by default. Override the
install directory with `UGOITE_INSTALL_DIR=/custom/bin` when needed.

Supported release artifacts currently target:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

Release archives use predictable names such as
`ugoite-v0.1.0-x86_64-unknown-linux-gnu.tar.gz` plus a matching `.sha256`
checksum file. Matching one-liner installer assets use predictable names such as
`ugoite-v0.1.0-x86_64-unknown-linux-gnu.install.sh`.

## Build from source (contributors)

Install dependencies from the repository root:

```bash
mise run //ugoite-cli:install
```

Alternatively, build directly in the CLI folder:

```bash
cd ugoite-cli
cargo build
```

Run the CLI from source with Cargo:

```bash
cargo run -q -p ugoite-cli -- --help
```

## Basic workflow

If you installed a released binary, use `ugoite` directly:

```bash
ugoite config current
ugoite config set --mode core

mkdir -p ./spaces
ugoite space list ./spaces
ugoite space create ./spaces/demo
```

The local filesystem examples in this section assume `core` mode. If you
previously pointed the CLI at a backend or API endpoint, `ugoite config current`
shows the saved routing state and `ugoite config set --mode core` switches back
to the local-first filesystem path before you create `./spaces/demo`.

If you are actively developing inside the repository, you can swap those for
`cargo run -q -p ugoite-cli -- ...` instead.

## Next steps after your first command

`./spaces/demo` is now your local workspace example. A good next step is to add
one plain Markdown entry there:

```bash
ugoite entry create ./spaces/demo first-note --content '# First note'
ugoite entry get ./spaces/demo first-note
```

You do **not** need Forms or sample data before this first note. Read
[Core Concepts](concepts.md) next if you want the mental model for spaces,
entries, forms, and search, or jump to
[Endpoint routing mode](#endpoint-routing-mode) when you want the CLI to talk to
a backend or API instead of the local filesystem.

## Command reference

Beyond the `space`, `entry`, `auth`, and `config` commands the quick-start
covers, the CLI ships five more top-level command families. The space-targeting
commands below use the same `SPACE_ID_OR_PATH` convention as the rest of the
CLI: pass `/root/spaces/<id>` in core mode and a bare `SPACE_ID` in
backend/api mode. `sql lint` is the one text-only helper here. Run `ugoite
config current` if you're not sure which mode you're in.

Every subcommand below has its own `--help` page (for example
`ugoite form list --help`) with full flag details.

### `ugoite form` - structured content types

Forms define the columns an entry can have (title, body, tags, custom fields).

- `ugoite form list <SPACE>` - list forms defined in the space
- `ugoite form get <SPACE> <FORM>` - show one form's schema
- `ugoite form update <SPACE> <FORM_FILE>` - upsert from a JSON file
- `ugoite form list-types` - list built-in column types you can compose

Example:

```bash
ugoite form list /root/spaces/demo
```

Use `form list-types` first to discover which column types you can put in your
schema, then `form update` to define a form, then pass `form: <FormName>` in
the entry frontmatter when you `entry create` so the new entry validates
against it.

### `ugoite search` - keyword search over entries

- `ugoite search keyword <SPACE> <QUERY>` - plain-text keyword search over indexed entry content

Example:

```bash
ugoite search keyword /root/spaces/demo "meeting notes"
```

Reach for `query` (below) when you need structured filters; reach for `search
keyword` when you want a plain-text match over entry content.

### `ugoite sql` - SQL tooling and saved queries

- `ugoite sql lint` - lint a SQL statement (currently a basic SELECT check)
- `ugoite sql saved-list` - list saved queries for a space
- `ugoite sql saved-get`, `saved-create`, `saved-update`, `saved-delete` - manage individual saved queries

Use this family to keep your SQL snippets alongside the space instead of a
shell history. Run `ugoite sql <subcommand> --help` for arguments.

### `ugoite index` - run and inspect the indexer

- `ugoite index run <SPACE>` - rebuild the search/query index from the
  current on-disk state of the space
- `ugoite index stats <SPACE>` - aggregated index stats (entry counts,
  form breakdown, last rebuild)

Run `index run` whenever you need to rebuild after an out-of-band edit (for
example a bulk change with another tool). `search keyword` and `query` both
read the indexer's output.

### `ugoite query` - run SQL over the index

```bash
ugoite query /root/spaces/demo --sql "SELECT id, title FROM entries LIMIT 10"
```

The queryable table is `entries`. Standard columns: `id`, `title`, `form`,
`updated_at`, `space_id`, `word_count`, `tags`. Form fields can also be
referenced directly by field name or through `properties.<field>`.

Filter by form type:

```bash
ugoite query /root/spaces/demo --sql "SELECT id, title FROM entries WHERE form='note'"
```

## Contributor-only shortcut: seed local sample data

If you installed a released CLI and only want the basic local workflow, you can
skip this section. `mise run seed` is a repository task for contributors and
repeatable demos, not a required first step for end users.

Use the root developer task when you want a quick local dataset without
remembering the lower-level CLI arguments:

```bash
mise run seed
```

The seed task creates `./spaces/dev-seed` by default, uses the
`renewable-ops` scenario, and loads 50 generated entries. To choose a
different target space, scenario, entry count, or deterministic RNG seed, set
the matching environment variables before running the task:

```bash
UGOITE_SEED_SPACE_ID=ux-demo UGOITE_SEED_SCENARIO=supply-chain UGOITE_SEED_ENTRY_COUNT=25 UGOITE_SEED_VALUE=42 mise run seed
mise run seed:scenarios
```

The seed flow prints a terminal progress bar while entries are generated. After
it finishes, confirm the seeded space:

```bash
cargo run -q -p ugoite-cli -- space list ./spaces
ls "./spaces/${UGOITE_SEED_SPACE_ID:-dev-seed}"
```

If you prefer the underlying direct command, the task is just a thin wrapper
over the Rust CLI and keeps Cargo builds inside the shared `target/rust` cache:

```bash
bash scripts/dev-seed.sh --space-id cli-demo --scenario lab-qa --entry-count 10 --seed 7
CARGO_TARGET_DIR=target/rust cargo run -q -p ugoite-cli -- space sample-data . cli-demo --scenario lab-qa --entry-count 10 --seed 7
CARGO_TARGET_DIR=target/rust cargo run -q -p ugoite-cli -- space sample-scenarios
```

## Notes

- The quick-start filesystem examples above assume `core` mode. Run
  `ugoite config current` to inspect the saved routing state and
  `ugoite config set --mode core` when you need to switch back from
  backend/api mode before using local paths such as `./spaces/demo`.
- In `core` mode, commands that target a specific space now take a positional
  `SPACE_ID_OR_PATH`. Pass the full local path such as
  `./spaces/dev-seed` or `/root/spaces/dev-seed`.
- `ugoite space list` takes the local root as a positional argument. Both
  `ugoite space list .` and `ugoite space list ./spaces` resolve to the same
  local workspace root.
- If `UGOITE_ROOT` is already exported for the local dev backend, `mise run seed`
  reuses that same root automatically so the seeded dataset stays visible to the
  local stack.
- If you use another directory, ensure it is writable and backed by local
  storage.

## Migration: core-mode space paths

Older examples used `--root <LOCAL_ROOT>` for visible `space` subcommands.
Those flows now use positional paths instead:

```bash
# Before
ugoite space create --root . demo
ugoite space get --root . demo
ugoite space patch --root . demo --name "Renamed"
ugoite space list --root .

# After
ugoite space create ./spaces/demo
ugoite space get ./spaces/demo
ugoite space patch ./spaces/demo --name "Renamed"
ugoite space list .
```

Backend and API modes still accept bare space IDs such as `demo`.

## Migration: hidden deprecated command names

Two older command names still exist only as hidden compatibility shims. Update
older snippets to the supported forms below instead of copying the deprecated
names forward:

| Hidden command | Replace it with | How to update old examples |
| --- | --- | --- |
| `ugoite create-space <SPACE_ID>` | `ugoite space create <SPACE_ID_OR_PATH>` | In `core` mode, pass the full local target such as `./spaces/demo` or `/root/spaces/demo`. In `backend` and `api` modes, keep using a bare space ID such as `demo`. Run `ugoite config current` first if you are unsure which mode is active. |
| `ugoite link create`, `ugoite link list`, `ugoite link delete` | Form-backed entry relationships using `row_reference` fields | Define the relationship in the Form itself, for example `{ "type": "row_reference", "target_form": "Project" }`, then create or update entries through the normal form-backed entry flow instead of managing standalone link records. |

If you still have old automation that uses `create-space`, these examples show
the exact migration:

```bash
# Before
ugoite create-space demo --root .
ugoite create-space demo

# After
ugoite space create ./spaces/demo
ugoite space create demo
```

## Endpoint routing mode

CLI can run in three modes, and stores the selection in `~/.ugoite/cli-endpoints.json`.

If you are choosing for the first time, use this rule of thumb:

| Mode | Choose it when | Practical trade-off |
| --- | --- | --- |
| `core` | You are working directly with a local checkout or local `spaces/` directory on this machine | Reads and writes your filesystem directly with no backend required. This is the default because it is the shortest local-first path. |
| `backend` | You want the CLI to talk to a backend server directly | Uses backend REST endpoints and the server's storage/auth behavior instead of your local filesystem. |
| `api` | You want the CLI to use the same proxied `/api` surface as the frontend | Follows the frontend-facing API path and its proxy/auth behavior instead of direct local access. |

Switch away from `core` only when you specifically want server-backed behavior
or the same frontend proxy path the browser uses.

When mode is `backend` or `api`, remote commands accept a `SPACE_ID` (or the
shared `SPACE_ID_OR_PATH` argument for commands that also work in `core` mode)
and perform filesystem I/O on the remote server instead of the local machine.

```bash
# Show the saved JSON config
cargo run -q -p ugoite-cli -- config show

# Show the active mode in plain language
cargo run -q -p ugoite-cli -- config current

# Route commands to backend directly
cargo run -q -p ugoite-cli -- config set --mode backend --backend-url http://localhost:8000

# Route commands to API endpoint
cargo run -q -p ugoite-cli -- config set --mode api --api-url http://localhost:3000/api

# Return to direct local filesystem access
cargo run -q -p ugoite-cli -- config set --mode core
```

Use `ugoite config current` whenever you want the same newcomer-facing summary
in plain language, including when the current mode is a better fit than the
other two and how to return to `core`.

## Auth profile commands

In `backend` / `api` modes, CLI and frontend share the same bearer token env convention:

- `UGOITE_AUTH_BEARER_TOKEN`
- `UGOITE_AUTH_API_KEY`

```bash
# Route auth commands to the backend
cargo run -q -p ugoite-cli -- config set --mode backend --backend-url http://localhost:8000

# Exchange username + 2FA for a bearer token in passkey-totp mode
cargo run -q -p ugoite-cli -- auth login --username dev-local-user --totp-code 123456

# Or use the explicit mock OAuth login path
cargo run -q -p ugoite-cli -- auth login --mock-oauth

# Inspect active auth setup, endpoint mode, and the next useful auth action
cargo run -q -p ugoite-cli -- auth profile

# Print unset commands
cargo run -q -p ugoite-cli -- auth token-clear
```

`ugoite auth login` saves a CLI-owned bearer-token session under the config home
(for example `~/.ugoite/cli-auth.json`) so follow-up `ugoite` commands stay
authenticated without `eval`. It still prints shell exports when you also want
the current shell to reuse the same token.

By default `ugoite auth login` and `ugoite auth token-clear` print POSIX
`export` / `unset` lines. When you use fish or PowerShell, request shell-native
output instead:

```fish
cargo run -q -p ugoite-cli -- auth login --shell fish --username dev-local-user --totp-code 123456 | source
```

```powershell
cargo run -q -p ugoite-cli -- auth token-clear --shell powershell | Invoke-Expression
```

The same `--shell` flag also works with `ugoite auth logout`.

`ugoite auth profile` distinguishes `core` mode (no backend credential required)
from `backend` / `api` modes. In server-backed modes it tells you whether a
bearer token or API key is already present, and whether the next step is
`ugoite auth login`, `ugoite auth token-clear`, or
`eval "$(ugoite auth token-clear)"` to clear the saved CLI session and apply the
printed credential unsets in your current shell.

In POSIX shells, `eval "$(ugoite auth token-clear)"` applies the printed
credential unsets to your current shell. In fish or PowerShell, use the
matching shell-native variant instead, such as
`ugoite auth token-clear --shell fish | source` or
`ugoite auth token-clear --shell powershell | Invoke-Expression`.

When the backend runs inside Docker/Compose and you target its published
backend port directly, export the matching `UGOITE_DEV_AUTH_PROXY_TOKEN` value
before `auth login` so the explicit local-login request stays trusted across the
container boundary.
