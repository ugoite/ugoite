# Ugoite SQL (Domain-Specific SQL)

**Version**: 0.1
**Updated**: 2026-01

Ugoite SQL provides a lightweight SQL dialect for querying Iceberg-backed entries.
It is designed for filtering and sorting entry records without changing API paths.

## Scope

- **Supported**: `SELECT *` with `FROM`, `WHERE`, `ORDER BY`, `LIMIT`, and `JOIN`.
- **Join support**: `INNER`, `LEFT`, `RIGHT`, `FULL`, and `CROSS` joins with
  `ON`, `USING`, and `NATURAL` constraints.
- **Not supported**: `GROUP BY`, `SELECT field projection`, subqueries,
  correlated subqueries.
- **Execution**: In-memory evaluation over records derived from Iceberg tables.
- **Safety limits**: Implementations MUST cap results to a server-side maximum
  (default 1000 rows) even when `LIMIT` is omitted.

## Materialized Views & Sessions

- Saved SQL (`create_sql`) **creates materialized view metadata** under
  `spaces/{space_id}/materialized_views/{sql_id}/`.
- Updates/deletes of saved SQL **refresh/remove** the corresponding metadata.
- SQL sessions store **metadata only** (no result rows); `view.snapshot_id` is a
  logical marker reserved for future materialized view support.
- Session metadata is stored under
  `spaces/{space_id}/sql_sessions/{session_id}/meta.json` and is short-lived
  (target: ~10 minutes).
- The design is **stateless beyond OpenDAL storage** (no RDB, no external
  job queue, no NFS shared disks).

## Tables

- `entries` — All entries across forms.
- `<FormName>` — Entries scoped to a specific form.
- `links` — Link rows (id, source, target, kind, source_form, target_form).
- `assets` — Asset rows (id, entry_id, name, path).

## Columns

- Standard columns: `id`, `title`, `form`, `updated_at`, `space_id`, `word_count`, `tags`.
- Form fields: Use field names directly (e.g., `Date`, `Owner`) or `properties.<field>`.
- Join columns: Use table-qualified names when joining (e.g., `n.id`, `l.target`).
- Complex join predicates (AND/OR, nested conditions) are supported.

## Saved SQL Form

Ugoite defines a system-owned **SQL** Form for persisting saved queries.
The SQL Form is a **metadata Form**; users cannot create Forms with the
reserved name `SQL`.

SQL Form fields:

- `sql` (markdown/string): SQL query text
- `variables` (object_list): JSON array of objects with `type`, `name`, and
  `description`

Saved SQL entries MUST embed every variable in the `sql` text using
`{{variable_name}}` placeholders. Placeholders MUST correspond to entries in
`variables`, and the SQL must be valid Ugoite SQL after substituting placeholders
with literal values.

## Examples

```sql
SELECT *
FROM entries
WHERE form = 'Meeting' AND updated_at >= {{since}}
ORDER BY updated_at DESC
LIMIT 50
```

```sql
SELECT * FROM Meeting WHERE Date >= '2025-01-01' AND tags = 'project'
```

```sql
SELECT * FROM entries WHERE properties.Owner = 'alice'
```

```sql
SELECT *
FROM entries n
JOIN links l ON n.id = l.source
WHERE l.kind = 'reference'
ORDER BY n.updated_at DESC
LIMIT 100
```

```sql
SELECT *
FROM entries n
RIGHT JOIN links l ON n.id = l.source
WHERE l.target = 'entry-2'
```

```sql
SELECT *
FROM entries n
FULL JOIN entries m USING (id)
WHERE n.id IS NOT NULL
```

## Errors

Invalid syntax or unsupported clauses must return an error from `ugoite-core` and
surface as a `400` or `500` response depending on the caller context.
Limits that exceed the server-side maximum must return a validation error.

## Lint & Completion Rules

Linting and completion rules are defined in the shared configuration file:

- `shared/sql/ugoite-sql-rules.json`

`ugoite-core` reads this file to provide CLI linting/completion behavior, while
the frontend uses the same file for editor hints without requiring runtime API
calls.

## Integration

Clients send SQL via the existing structured query payload:

```json
{
  "filter": {
    "$sql": "SELECT * FROM entries WHERE form = 'Meeting'"
  }
}
```
