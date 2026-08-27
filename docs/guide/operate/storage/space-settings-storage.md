---
title: "Space settings and storage"
sidebar:
  order: 2
---

A Space remains operator-owned and is reached through its configured OpenDAL
backend. REST supports list/create/get/patch/test-connection operations; Space
creation requires the Node administrator role. A Space is the portable move
unit; Node control state and the node secret are separate node recovery inputs.

For a non-local binding, Ugoite verifies the storage behavior when the server
opens the active operator. The verified shared mode requires exact reads and
conditional create/replace behavior, including stale-revision rejection and a
single winner for concurrent CAS. A readable binding that cannot prove those
writes is exposed as `SharedReadOnly`; an exact-read failure is unavailable.
The health response includes the selected mode and a safe probe reason.

Bindings contain only non-secret operator settings such as URI and optional
custom endpoint. They are node-local locator metadata; the active server
operator is opened from deployment configuration (`UGOITE_ROOT` and, when
needed, `UGOITE_STORAGE_ENDPOINT`). Credentials come from the operator
environment, workload identity, or provider credential chain and are never
persisted by Ugoite.

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
