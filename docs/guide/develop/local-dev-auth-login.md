---
title: Local development authentication
sidebar:
  order: 2
---

> Future/reference workflow: Passkey/TOTP authentication is not part of the
> supported v0.1 product boundary. This page describes planned server setup for
> development only.

Run `mise run dev`, then read the one-use setup URL printed by `ugoite-server`.
The default local RP is `localhost` with origin `http://localhost:8000`. If a
proxy or alternate port is used, set both values before first registration:

```bash
export UGOITE_PUBLIC_ORIGIN=http://localhost:3000
export UGOITE_WEBAUTHN_RP_ID=localhost
mise run dev
```

Open the setup URL, register a real platform Passkey, and save the displayed
recovery codes. Complete setup with a second Passkey or confirmed TOTP before
continuing. Subsequent browser logins use `/login`. CLI development uses the
same device authorization flow as production:

```bash
ugoite config set --mode backend --backend-url http://localhost:8000
ugoite auth login --actions read,create,update
```

There is no local authentication bypass or default credential. Supply a stable
`UGOITE_NODE_SECRET_KEY` or `UGOITE_NODE_SECRET_FILE` before startup. To repeat
first-run setup, use a new empty `UGOITE_ROOT`. Preserve existing Space prefixes
as complete operator-controlled copies; current setup claims supported Spaces and
fails explicitly for older or incomplete layouts.
