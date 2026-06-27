---
title: 'Admin Space operations'
---

`admin-space` is the reserved authorization boundary for deployment-wide Space administration.

`POST /spaces` requires `ManageSpace` permission on `admin-space`. The public REST API does not grant that permission or bootstrap `admin-space` for an unauthenticated caller.

For a fresh deployment, use startup bootstrap settings to create an initial usable Space and owner membership:

```text
UGOITE_BOOTSTRAP_DEFAULT_SPACE=true
UGOITE_DEV_USER_ID=<initial-user-id>
```

Treat `admin-space` membership as privileged. Back up the complete workspace before recovery work and prefer supported core/CLI paths over editing authorization files by hand.
