---
title: "Log redaction"
sidebar:
  order: 4
---

Never log credentials or sensitive payloads. Redact:

- authorization/API-key headers;
- setup and invitation links, access and refresh credentials, and encryption
  keys;
- opaque session cookies and OAuth credentials;
- owner-recovery approval tokens, replacement recovery codes, TOTP secrets, and
  their hashes;
- complete Entry bodies, assets, or SQL results unless deliberately enabled for
  local debugging;
- local paths that expose operator or tenant details.

Prefer operation name, request ID, Space ID, status, duration, and bounded error
metadata.

Owner recovery responses are `Cache-Control: no-store`. Audit records contain
only tuple identifiers, operation IDs, outcome, and `audit_status`; a pending
audit delivery must never cause a one-time token or code response to be
replayed.
