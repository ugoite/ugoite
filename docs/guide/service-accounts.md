# Service accounts

Managed service-account CRUD is **not implemented** in the current Rust server or CLI. The checked-in OpenAPI document contains no `/service-accounts` routes.

For unattended access in this release, provision credentials out of band through server configuration:

- `UGOITE_AUTH_API_KEYS` for static API keys;
- `UGOITE_AUTH_BEARER_TOKENS` for static bearer tokens;
- signing-secret variables for signed bearer validation.

Grant the associated identity only the required Space membership, rotate configuration deliberately, restart the service, and never commit secrets. Managed creation, rotation, revocation, and audit history remain future work.
