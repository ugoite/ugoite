---
title: "Space settings and storage"
sidebar:
  order: 2
---

A Space remains operator-owned and is reached through its configured OpenDAL
backend. REST supports list/create/get/patch/test-connection operations; Space
creation requires the Node administrator role. A Space is the portable move
unit; Node control state and the node secret are separate node recovery inputs.

When moving storage:

1. stop or quiesce writers;
2. while writes are stopped, copy the complete Space prefix, including Catalog
   Head, publication evidence, Iceberg metadata/manifests/data, and
   metadata/history, or take the backend's native consistent snapshot;
3. preserve permissions for the non-root runtime user (`10001` in supplied
   container/chart defaults);
4. update configuration;
5. run the connection test and representative reads before deleting the old
   copy.

Do not move only a derived index or rebuild Iceberg metadata from a listing. Do
not move the Node control-store prefix as part of the Space move.
