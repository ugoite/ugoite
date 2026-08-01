---
title: 'Ugoite SQL'
---

Ugoite exposes two related surfaces:

1. **Saved SQL** under `/spaces/{space_id}/sql`, stored as versioned content in the Space.
2. **SQL sessions** under `/spaces/{space_id}/sql-sessions`, which store expiring query metadata and rerun queries to return count/rows.

The CLI `ugoite query <space> --sql ...` executes directly in core mode or creates/reads a remote SQL session in backend/API mode. SQL is parsed, planned, optimized, and executed by DataFusion over Iceberg Form providers. Each Form is exposed under its lowercase name with its Form fields plus the reserved system columns `_ugoite_id`, `_ugoite_title`, `_ugoite_created_at`, and `_ugoite_updated_at`; there is no cross-Form aggregate relation.

Only one read-only DataFusion statement is accepted. The query context exposes only
authorized Form relations and explicitly allowlisted functions, then applies
Iceberg/DataFusion projection, predicate, and limit pushdown. Unsupported
relations, functions, and statement kinds fail without a compatibility fallback.

Parameters use DataFusion's native `$name` syntax and are bound as typed
DataFusion scalar values before planning. Ugoite never substitutes parameter
text into SQL.

SQL sessions do not become authoritative data. Their metadata is stored below
`sql_sessions/`; result rows are regenerated from the session's fixed,
publication-verified `SpaceCheckpoint`, never from the live Space Head. The
initial pagination contract accepts only a simple single-Form `SELECT` whose
explicit `ORDER BY` ends in `_ugoite_id`, which is the Form's stable unique tie
breaker. Joins, aggregates, `DISTINCT`, subqueries, and queries without that
total order are rejected rather than receiving an unstable cursor protocol.

Each session stores the creating principal set, a canonical fingerprint of the
Entry access policies, and a derived query policy beside (not inside) its
checkpoint. Session creation validates the SQL shape before it resolves the
one requested Form at that checkpoint. Its provider boundary carries sparse
Entry denials rather than a Rust-collected list of every readable Entry. Every
status, count, and page request revalidates the current non-empty principal
contract and that fingerprint without rebuilding Entry scope or Form metadata
from the live Head. A policy change requires a new session; an ordinary data
write, Form evolution, or unrelated authorization activity does not move an
existing session away from its checkpoint.
