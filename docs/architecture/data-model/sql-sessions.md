---
title: "SQL sessions"
---

The implementation persists query metadata through OpenDAL and re-evaluates
results from one fixed immutable publication. It does not require an RDB, queue,
or shared filesystem beyond the configured storage operator.

## Session metadata

`POST /spaces/{space_id}/sql-sessions` creates:

```text
spaces/{space_id}/sql_sessions/{session_id}/meta.json
```

The file contains:

```json
{
  "id": "session-uuid",
  "space_id": "space-main",
  "sql_id": "saved-or-generated-sql-id",
  "sql": "SELECT * FROM note ORDER BY _ugoite_updated_at, _ugoite_id",
  "parameters": {},
  "parameter_types": {},
  "authorized_principal_ids": ["principal-uuid"],
  "authorization_policy_hash": "sha256:…",
  "status": "ready",
  "created_at": "2026-03-11T10:10:00Z",
  "expires_at": "2026-03-11T10:20:00Z",
  "error": null,
  "publication": {
    "generation": 42,
    "publication_uri": {
      "space_uid": "019c1234-5678-7abc-8def-0123456789ab",
      "key": "_ugoite/catalog/publications/42-command.json"
    },
    "publication_checksum": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  },
  "query_policy": {
    "forms": [{
      "form_id": "form-uuid",
      "relation": "note",
      "entry_scope": { "all_except": ["entry-hidden-from-session"] },
      "columns": ["Body"],
      "system_columns": ["external_id", "title", "created_at", "updated_at"]
    }]
  },
  "pagination": {
    "strategy": "offset",
    "total_order": "ORDER BY ending with _ugoite_id",
    "default_limit": 50,
    "max_limit": 1000,
    "max_offset": 999
  },
  "limits": {
    "max_rows": 1000,
    "max_memory_bytes": 67108864,
    "timeout_ms": 30000,
    "max_concurrency": 1
  },
  "count": { "mode": "on_demand", "cached_at": null, "value": null }
}
```

Rows are not stored in the session directory. Creation parses and validates the
single Form relation before it resolves a publication, then freezes that Form's
publication metadata, columns, system columns, and provider-side Entry scope.
When every principal has Space read access, the scope records only Entry-level
denials as `all_except`; it never enumerates every readable Entry in metadata.
Any explicit or sparse scope is capped at the session's 1,000-row hard limit and
is rejected before session metadata is written when it exceeds that bound.
`query_policy` is derived metadata, never execution authority. Status, count,
and paged-row requests reparse the stored SQL, resolve the one Form from the
publication's immutable metadata, and rebuild its scope, columns, and system
columns from the current authorization state before comparing that expected
policy with the stored cache. DataFusion receives the rebuilt policy, never a
policy accepted solely from OpenDAL metadata. The principal set and a canonical
fingerprint of Entry access policies must still match the session; current
principal activity, sponsorship, and expiry are checked separately. A policy
change is rejected and requires a new session. An empty principal set is
forbidden. There is no stream endpoint.

The initial page surface accepts only one Form relation and an explicit total
order ending in `_ugoite_id`. The API rejects joins, aggregates, `DISTINCT`,
subqueries, and page ranges beyond the retained 1,000-row session window. Counts
and pages remain bounded by the same DataFusion memory, timeout, and
single-query concurrency limits.

Sessions expire after ten minutes by default. Accessing an expired session
derives `status: "expired"` in memory and returns an expiration error; reads do
not persist status changes. Expiry is a logical access lifetime only: metadata
remains physically retained in OpenDAL until an operator performs documented
cleanup. A periodic cleanup worker is not shipped.
