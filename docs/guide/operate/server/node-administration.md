---
title: Node administration
sidebar:
  order: 3
---

> Limited administration workflow. OIDC provider create/list/disable APIs are
> supported v0.1 capabilities and require NodeAdmin plus a recent Passkey.

This page intentionally does not document manual identity-file editing. Use
the REST API and `/openapi.json` as the source of truth; do not edit
authorization or identity JSON by hand. Provider disable is a soft disable:
existing links remain, while new and in-flight OIDC attempts are rejected.
