# IEapp SQL (Domain-Specific SQL)

**Version**: 0.1
**Updated**: 2026-01

IEapp SQL provides a lightweight SQL dialect for querying Iceberg-backed notes.
It is designed for filtering and sorting note records without changing API paths.

## Scope

- **Supported**: `SELECT *` with `FROM`, `WHERE`, `ORDER BY`, `LIMIT`.
- **Not supported**: `JOIN`, `GROUP BY`, `SELECT field projection`, subqueries.
- **Execution**: In-memory evaluation over note records derived from Iceberg tables.

## Tables

- `notes` — All notes across classes.
- `<ClassName>` — Notes scoped to a specific class.

## Columns

- Standard columns: `id`, `title`, `class`, `updated_at`, `workspace_id`, `word_count`, `tags`.
- Class fields: Use field names directly (e.g., `Date`, `Owner`) or `properties.<field>`.

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

## Errors

Invalid syntax or unsupported clauses must return an error from `ieapp-core` and
surface as a `400` or `500` response depending on the caller context.

## Integration

Clients send SQL via the existing structured query payload:

```json
{
  "filter": {
    "$sql": "SELECT * FROM notes WHERE class = 'Meeting'"
  }
}
```
