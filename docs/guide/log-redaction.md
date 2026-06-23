# Log redaction

Never log credentials or sensitive payloads. Redact:

- authorization/API-key headers;
- bootstrap tokens, static credentials, and signing secrets;
- session cookies and signed tokens;
- complete Entry bodies, assets, or SQL results unless deliberately enabled for local debugging;
- local paths that expose operator or tenant details.

Prefer operation name, request ID, Space ID, status, duration, and bounded error metadata. Development mock OAuth follows the same rule.
