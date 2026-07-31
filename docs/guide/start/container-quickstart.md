---
title: Container quick start
sidebar:
  order: 3
---

```bash
export UGOITE_VERSION=0.1.0
export UGOITE_NODE_SECRET_KEY="$(head -c 32 /dev/urandom | base64)"
docker compose -f docker-compose.release.yaml up -d
docker compose -f docker-compose.release.yaml logs ugoite
```

Open the one-use setup URL shown in the log and register a Passkey. Complete
setup with a second Passkey or TOTP plus the displayed recovery codes. The URL
expires after 30 minutes and can be used once. The local runtime data directory
is `${UGOITE_DATA_DIR:-./data}`. In this default local layout, Space storage and
the default Node control store live below that directory. The example's
`UGOITE_NODE_SECRET_KEY` is supplied by the environment, so preserve its value
separately; it is not included in a data-directory copy.

For a remote hostname, configure the WebAuthn origin before first start:

```bash
export UGOITE_PUBLIC_ORIGIN=https://ugoite.example.com
export UGOITE_WEBAUTHN_RP_ID=ugoite.example.com
docker compose -f docker-compose.release.yaml up -d
```

HTTPS is mandatory for non-localhost Passkeys and Secure session cookies.
