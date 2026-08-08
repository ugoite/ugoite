---
title: Node administration
sidebar:
  order: 3
---

Node administration is a Node Identity role and is separate from every Space. A
Node administrator configures OIDC, manages account status, creates Spaces,
establishes Node-local bindings during setup, and operates the server. Space owners
manage membership, agents, ACLs, and data only within their Space.

`POST /spaces` requires `node_admin`. A newly created Space receives a distinct
owner principal bound to the creating account. Never edit Node Identity or Space
authorization JSON by hand; use the authenticated management surfaces. For
recovery, back up the complete Space prefix, the configured Node control-store
prefix, and the node secret as separate inputs.
