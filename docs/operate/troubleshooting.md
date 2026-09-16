---
title: "Troubleshooting"
description: Symptom-first diagnosis that keeps Knowledge safe.
sidebar:
  order: 8
---

Start from the symptom and preserve the recovery inputs while diagnosing. Do
not delete Space data, rewrite a Catalog Head, or rotate the node secret as a
first response.

## Cannot sign in

**Check:** Confirm the public origin and WebAuthn RP ID, then retry with a
registered Passkey. After a restore, confirm the same Node control-store prefix
and node secret are present.

**Recovery:** Use the supported Account Self-Recovery or owner-approved Space
access recovery flow when its prerequisites are met. TOTP alone is not a login
method.

**What remains durable?** Space content, history, membership, and ACL state are
untouched by a failed login. Node-local sessions and challenges may expire.

## Cannot open a Space

**Check:** Run `ugoite config current`. In core mode use the local Space path;
in backend/API mode use the immutable Space UID. Confirm the account has Space
membership and the requested action.

**Recovery:** Re-authenticate only after checking the endpoint. Ask the Space
owner to inspect membership or use the owner-approved recovery flow; re-login
cannot repair a missing binding or role by itself.

**What remains durable?** The complete Space prefix and other Spaces remain
unchanged even when the current identity lacks access.

## Entry cannot be saved

**Check:** Confirm the Form name and typed values. For an update, read the
  newest revision and pass its parent revision when the command or API requires
  optimistic concurrency.

**Recovery:** Correct the input and retry. A failed validation or stale-parent
  request does not overwrite the previous revision.

**What remains durable?** Prior Entry revisions and the Form definition remain
readable; only a successfully committed mutation creates a new revision.

## Search does not show a recent change

**Check:** Confirm the Entry save receipt, Space, and Form. Rerun the keyword or
structured search, and inspect index status if the authoritative read succeeds.

**Recovery:** Rebuild the supported derived index after the Space opens. Do not
delete authoritative Entry or Asset objects to repair search.

**What remains durable?** A committed Entry and its revision history remain
durable even when a derived search index is stale or missing.

## Revision conflict

**Check:** Read Entry history and identify the newest revision.

**Recovery:** Reapply the intended edit against that revision. Do not force a
stale update or rewrite old history.

**What remains durable?** No prior revision is overwritten; the conflict is an
explicit protection of append-only history.

## Storage mutation unavailable

**Check:** Verify `/health`, configured storage connectivity, permissions on the
  mounted path, and whether the complete Space prefix is reachable. For remote
  mutations, confirm the authenticated identity has the required Space action.

**Recovery:** Stop writes, preserve the original prefix and node secret, and
  use [Storage and Recovery](storage-recovery.md) to restore or test a complete
  backend. Never rebuild a Catalog Head from an object listing.

**What remains durable?** Data already committed in the authoritative Space
  remains durable. An interrupted mutation must be checked by reading history
  and the operation receipt before retrying.

## Server does not start

**Check:** Run `docker compose ps`, read `docker compose logs ugoite`, and
  confirm the `/data` mount, required node secret, public origin, and listening
  port. Use [Health and Diagnostics](health-diagnostics.md) for the smallest
  readiness check.

**Recovery:** Correct the deployment configuration and restart against the same
  recovery inputs. Do not initialize a new data root just to bypass a startup
  error.

**What remains durable?** The original Space, control store, and secret remain
  recoverable while their complete prefixes are preserved.

## Invalid saved CLI config

The CLI fails closed on an unreadable or invalid saved endpoint config. It
reports the config path and the cause, exits non-zero, and never silently
falls back to core mode.

**Check:** Run any CLI command and read the reported path and cause:

```text
cannot load CLI configuration at /home/you/.ugoite/cli-endpoints.json: configuration file contains invalid JSON
```

```text
cannot load CLI configuration at /home/you/.ugoite/cli-endpoints.json: configuration contains invalid endpoint configuration
```

A missing config file is not an error: the CLI uses the default core-mode
endpoint. Only an unreadable file, invalid JSON, an invalid endpoint
configuration, or an invalid server URL fails this way. The saved config
path is, in order, `$UGOITE_CLI_CONFIG_PATH`, `$UGOITE_CONFIG_HOME/ugoite/cli-endpoints.json`,
`$XDG_CONFIG_HOME/ugoite/cli-endpoints.json`, then `~/.ugoite/cli-endpoints.json`.

**Recovery:** Explicitly choose a valid config, profile, or path, or create
a new valid config. Do not delete Space data or rotate secrets to fix a CLI
config error.

1. Confirm which path the error reported, and inspect it with
   `ugoite config show`. If the reported path is overridden by the
   environment, decide which location should win before editing.
2. Create a new valid config or point at a known-good one:

```bash
ugoite config set --mode core
ugoite config set --mode backend --backend-url http://localhost:8000
ugoite config set --mode api --api-url https://example.com/api
```

```bash
UGOITE_CLI_CONFIG_PATH=/path/to/known-good/cli-endpoints.json ugoite config current
```

3. Confirm the active target before sending requests:

```bash
ugoite config current
ugoite config show
ugoite auth profile
```

`ugoite config current` names the active endpoint mode in plain language;
`ugoite config show` prints the saved endpoint JSON. In backend or API mode,
use the immutable Space UID and a valid device credential; in core mode, use
the local Space path. Never assume core mode after a config error: the
failure means the active target is unknown until `config current` confirms
it.

**What remains durable?** The Space prefix, its history, and the node
control state are untouched by a CLI config failure. Only local CLI routing
is unknown until recovery completes.

## Related

- [Health and Diagnostics](health-diagnostics.md)
- [Identity and Access](identity-access.md)
- [Storage and Recovery](storage-recovery.md)
- [Configure](configure.md)
