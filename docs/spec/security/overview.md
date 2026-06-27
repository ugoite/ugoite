---
title: "Security overview"
---

This overview summarizes the security controls implemented today and the
limitations that deployments must account for.

## Current controls

- server defaults to loopback unless deployment configuration changes it;
- supplied container/chart run as non-root and use private persistent storage;
- bearer tokens, API keys, signed tokens, and session cookies authenticate
  requests;
- Space membership and roles authorize reads, content writes, membership
  operations, and administration;
- IDs are validated before storage access;
- local Space directories and sensitive files use owner-only permissions on
  Unix;
- request bodies are limited to 20 MiB;
- MCP output frames Entry content as untrusted and sanitizes unsafe HTML/script
  content;
- integrity keys and append-only revisions protect stored content history.

## Current limitations

Passkey/TOTP login is not implemented even though the route is contracted.
Managed service-account lifecycle, scope-enforced identities, and audit-log APIs
are unavailable. Development mock OAuth must not be treated as production
authentication.

Secrets must be injected through deployment configuration, never committed or
logged.
