---
title: 'Troubleshooting unauthorized Spaces'
---

HTTP `401` means no valid identity; HTTP `403` means the identity lacks the required permission.

1. Check `ugoite config current`.
2. Check `ugoite auth profile` or `GET /auth/session`.
3. Use a bare Space ID in backend/API mode and a filesystem path in core mode.
4. Confirm membership and role for the identity.
5. For Space creation, confirm `ManageSpace` on `admin-space`.

Development mock OAuth does not bypass membership checks. Re-authenticating as the same user cannot repair a missing role.
