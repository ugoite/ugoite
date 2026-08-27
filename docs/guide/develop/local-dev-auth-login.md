---
title: Local development authentication
sidebar:
  order: 2
---

> Supported local server workflow. Passkey/WebAuthn bootstrap and passwordless
> browser login and invitation-gated OIDC login are part of the v0.1 contract;
> Account Self-Recovery and device login require their documented setup.

Start the local server, open its one-use setup URL, register the initial
Passkey, then use the browser login page. The local CLI core workflow remains
available without a server. Do not treat development configuration as an
authentication bypass.
