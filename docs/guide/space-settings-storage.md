---
title: "Space settings and storage"
---

A Space lives below `UGOITE_ROOT` and remains operator-owned. REST supports
list/create/get/patch/test-connection operations; Space creation requires the
Node administrator role.

When moving storage:

1. stop or quiesce writers;
2. copy the complete Space directory, including metadata/history;
3. preserve permissions for the non-root runtime user (`10001` in supplied
   container/chart defaults);
4. update configuration;
5. run the connection test and representative reads before deleting the old
   copy.

Do not move only the derived index; the complete directory is the portable unit.
