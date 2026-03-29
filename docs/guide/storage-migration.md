# Storage Migration Guide

Use this guide before you change a space from one storage URI to another.

## What changing the URI does today

Updating a space's `storage_config.uri` changes the saved connector settings for
that space metadata. It does **not** automatically copy existing entries, assets,
or derived indexes from the old location to the new one.

That means a storage switch is currently a **manual migration step**, not a
one-click move.

## Practical checklist before switching

1. Confirm why you are changing backends.
   - `file://` keeps the space local-first on the current machine.
   - `s3://` changes the trust, cost, and credential model to cloud object
     storage.
2. Validate the destination first with **Test Connection** in Space Settings.
3. Copy the existing space data with tooling that matches your current storage
   backend.
4. Update the Storage URI only after you know the new location is reachable.
5. Verify that the space opens correctly from the new backend before treating the
   migration as complete.

## Local vs object-storage trade-offs

### `file://`

- best for local-first ownership and offline access
- simplest operational model
- tied to the current machine or mounted filesystem unless you copy it elsewhere

### `s3://`

- useful when you need remote object storage durability or sharing workflows
- depends on cloud credentials, network reachability, and storage costs
- changes the operational boundary away from purely local storage

## Recommended safety checks

- Keep a backup of the old location until you have verified the new one.
- Treat URI changes as topology changes, not cosmetic edits.
- Document which environment owns the new credentials and access policy.

## Related references

- [Space Settings storage UI](../../frontend/src/components/SpaceSettings.tsx)
- [REST API: spaces and test connection](../spec/api/rest.md#spaces)
