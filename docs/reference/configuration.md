---
title: "Configuration"
description: Exact deployment variables and configuration entry points.
sidebar:
  order: 5
---

Operators configure deployments through environment variables after selecting a
runtime shape. This page lists the current names and defaults; the procedure for
choosing and changing them is [Operate / Configure](../operate/configure.md).

## Server variables

| Variable | Purpose | Default or requirement |
| --- | --- | --- |
| `UGOITE_ROOT` | Space storage and, by default, Node control state | `./data` |
| `UGOITE_SERVER_ADDRESS` | HTTP listen address | `127.0.0.1:8000` |
| `UGOITE_STATIC_DIR` | Compiled browser directory | unset |
| `UGOITE_PUBLIC_ORIGIN` | Public WebAuthn/OAuth origin | `http://localhost:8000` |
| `UGOITE_API_BASE_URL` | Integrated browser API base, including `/api` | public origin plus `/api` |
| `UGOITE_WEBAUTHN_RP_ID` | WebAuthn relying-party ID | public-origin host |
| `UGOITE_NODE_CONTROL_URI` | Separate OpenDAL prefix for Node control state | `UGOITE_ROOT` storage |
| `UGOITE_NODE_SECRET_KEY` | Encryption root key | required unless secret file is set |
| `UGOITE_NODE_SECRET_FILE` | Mounted file containing the encryption root | unset |
| `UGOITE_CORS_ALLOWED_ORIGINS` | Exact credentialed browser origins | disabled when unset |
| `UGOITE_STORAGE_ENDPOINT` | Optional storage service endpoint | backend-specific |

`UGOITE_NODE_SECRET_KEY` must contain at least 32 random bytes. A deployment
must preserve the same encryption root across restarts. It is not a password,
API key, bearer token, or browser credential.

Remote deployments must use an HTTPS public origin. The RP ID must be a valid
registrable suffix of the origin host and must be configured before Passkey
registration. CORS is disabled by default; when enabled, list exact origins and
keep the canonical origin/CSRF rules in place.

## Compose entry points

The release file reads `UGOITE_VERSION`, `UGOITE_PORT`, and `UGOITE_DATA_DIR` in
addition to the server variables above. Source Compose uses the same runtime
variables but builds the image locally. Both mount the configured data root at
`/data` inside the container.

## Frontend and CLI variables

`BACKEND_URL` selects the frontend's server proxy target during development.
The CLI keeps disposable work-environment state separately: canonical TOML in
`./.ugoite/config.toml` (project-local, preferred when present),
`$UGOITE_CONFIG` (platform-separated list), then `~/.ugoite/config.toml`, with
secrets only in `~/.ugoite/credentials.json`; use `ugoite config current` and
`ugoite context --help` rather than treating server environment variables
as CLI flags. The legacy single-mode file (`$UGOITE_CLI_CONFIG_PATH`,
`$UGOITE_CONFIG_HOME/ugoite/cli-endpoints.json`,
`$XDG_CONFIG_HOME/ugoite/cli-endpoints.json`, then
`~/.ugoite/cli-endpoints.json`) remains readable in v0.1.x; `ugoite config
migrate` normalizes it to canonical TOML without touching Knowledge. An
invalid saved CLI config fails closed with the reported path
and cause; recover with an explicit valid config and confirm with
`ugoite config current` before requests, never with a silent fallback. See
[invalid saved CLI config recovery](../operate/troubleshooting.md#invalid-saved-cli-config).

Model-assisted local Work may use `UGOITE_MODEL_API_KEY`,
`UGOITE_MODEL_BASE_URL`, `UGOITE_MODEL_NAME`, and
`UGOITE_MODEL_TIMEOUT_SECS`. These configure a provider interaction; they do
not create Knowledge storage or authorization state.

## Recovery and authority

Back up the complete Space prefix, the complete Node control-store prefix when
separate, and the node secret as three identifiable recovery inputs. A `/data`
snapshot is complete only when it contains the configured storage inputs and the
secret is preserved with it. See [Storage and Recovery](../operate/storage-recovery.md)
and the [Space compatibility contract](../architecture/contracts/space-compatibility.md).

This reference describes names and meaning. Runtime parsing and defaults remain
owned by `ugoite-server`, the chart, Compose files, and the executable help.
