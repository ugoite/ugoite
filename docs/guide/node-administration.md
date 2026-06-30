---
title: Node administration
---

Node administration is a Node Identity role and is separate from every Space. A
Node administrator configures OIDC, manages account status, creates Spaces,
performs owner rebinding after migration, and operates the server. Space owners
manage membership, agents, ACLs, and data only within their Space.

`POST /spaces` requires `node_admin`. A newly created Space receives a distinct
owner principal bound to the creating account. Never edit Node Identity or Space
authorization JSON by hand; use the authenticated management surfaces and back
up the complete operator-owned data root.
