---
title: "Troubleshooting unauthorized Spaces"
sidebar:
  order: 3
---

HTTP `401` means no valid identity; HTTP `403` means the identity lacks the
required permission.

1. Check `ugoite config current`.
2. Check `ugoite auth profile` or `GET /auth/session`.
3. Use a bare Space ID in backend/API mode and a filesystem path in core mode.
4. Confirm membership and role for the identity.
5. For Space creation, confirm the account has the Node administrator role;
   Space ownership does not grant it.

Re-authenticating as the same account cannot repair a missing Space binding,
role, or resource grant. Ask a Space owner to inspect the principal state.
