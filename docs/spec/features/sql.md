# IEapp SQL (Domain-Specific SQL)

**Version**: 0.1
**Updated**: 2026-01

IEapp SQL provides a lightweight SQL dialect for querying Iceberg-backed notes.
It is designed for filtering and sorting note records without changing API paths.

## Scope

- **Supported**: `SELECT *` with `FROM`, `WHERE`, `ORDER BY`, `LIMIT`, and `JOIN`.
- **Join support**: `INNER`, `LEFT`, `RIGHT`, `FULL`, and `CROSS` joins with
  `ON`, `USING`, and `NATURAL` constraints.
- **Not supported**: `GROUP BY`, `SELECT field projection`, subqueries,
  correlated subqueries.
- **Execution**: In-memory evaluation over records derived from Iceberg tables.
- **Safety limits**: Implementations MUST cap results to a server-side maximum
  (default 1000 rows) even when `LIMIT` is omitted.

## Tables

- `notes` — All notes across classes.
- `<ClassName>` — Notes scoped to a specific class.
- `links` — Link rows (id, source, target, kind, source_class, target_class).
- `attachments` — Attachment rows (id, note_id, name, path).

## Columns

- Standard columns: `id`, `title`, `class`, `updated_at`, `workspace_id`, `word_count`, `tags`.
- Class fields: Use field names directly (e.g., `Date`, `Owner`) or `properties.<field>`.
- Join columns: Use table-qualified names when joining (e.g., `n.id`, `l.target`).
- Complex join predicates (AND/OR, nested conditions) are supported.

## Examples

```sql
SELECT * FROM notes WHERE class = 'Meeting' ORDER BY updated_at DESC LIMIT 50
```

```sql
SELECT * FROM Meeting WHERE Date >= '2025-01-01' AND tags = 'project'
```

```sql
SELECT * FROM notes WHERE properties.Owner = 'alice'
```

```sql
SELECT *
FROM notes n
JOIN links l ON n.id = l.source
WHERE l.kind = 'reference'
ORDER BY n.updated_at DESC
LIMIT 100
```

```sql
SELECT *
FROM notes n
RIGHT JOIN links l ON n.id = l.source
WHERE l.target = 'note-2'
```

```sql
SELECT *
FROM notes n
FULL JOIN notes m USING (id)
WHERE n.id IS NOT NULL
```

## Errors

Invalid syntax or unsupported clauses must return an error from `ieapp-core` and
surface as a `400` or `500` response depending on the caller context.
Limits that exceed the server-side maximum must return a validation error.

## Lint & Completion Rules

Linting and completion rules are defined in the shared configuration file:

- `shared/sql/ieapp-sql-rules.json`

`ieapp-core` reads this file to provide CLI linting/completion behavior, while
the frontend uses the same file for editor hints without requiring runtime API
calls.

## Integration

Clients send SQL via the existing structured query payload:

```json
{
  "filter": {
    "$sql": "SELECT * FROM notes WHERE class = 'Meeting'"
  }
}
```
