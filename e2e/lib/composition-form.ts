export type CompositionFormSeedField = {
  label?: string;
  sql_column?: string;
  query_capability?: { projectable?: boolean };
};

/** Expected Form seed for E2E fixtures whose API field order differs by surface. */
export function expectedCompositionFormSeedSql(
  relation: string,
  fields: Record<string, CompositionFormSeedField> | undefined,
): string {
  const aliases = new Set<string>();
  const columns = Object.entries(fields ?? {})
    .flatMap(([name, field]) => {
      const column = field.sql_column?.trim();
      if (
        !column || column.startsWith("_ugoite_") ||
        field.query_capability?.projectable !== true
      ) return [];

      const base = field.label?.trim() || name;
      let alias = base;
      let suffix = 2;
      while (aliases.has(alias)) alias = `${base} (${suffix++})`;
      aliases.add(alias);
      return [`${sqlIdentifier(column)} AS ${sqlIdentifier(alias)}`];
    })
    .sort();
  if (columns.length === 0) {
    throw new Error("Form seed expectations need a projectable field");
  }
  return `SELECT ${columns.join(", ")} FROM ${
    sqlIdentifier(relation)
  } ORDER BY "_ugoite_updated_at" DESC, "_ugoite_id"`;
}

/**
 * Normalize only projection order for REST fixture comparisons. The UI's
 * user-visible field order is asserted by CompositionStudioSync tests.
 */
export function normalizeCompositionFormSeedSql(sql: string): string {
  if (!sql.startsWith("SELECT ")) {
    throw new Error("Form seed SQL must start with SELECT");
  }
  const remainder = sql.slice("SELECT ".length);
  const fromIndex = findSqlKeyword(remainder, " FROM ");
  if (fromIndex < 0) throw new Error("Form seed SQL is missing FROM");

  const projections = splitSqlList(remainder.slice(0, fromIndex)).sort();
  if (projections.length === 0) {
    throw new Error("Form seed SQL needs a projection");
  }
  return `SELECT ${projections.join(", ")}${remainder.slice(fromIndex)}`;
}

function findSqlKeyword(sql: string, keyword: string): number {
  let quoted = false;
  for (let index = 0; index <= sql.length - keyword.length; index += 1) {
    if (sql[index] === '"') {
      if (quoted && sql[index + 1] === '"') {
        index += 1;
        continue;
      }
      quoted = !quoted;
    }
    if (!quoted && sql.startsWith(keyword, index)) return index;
  }
  return -1;
}

function splitSqlList(projections: string): string[] {
  const columns: string[] = [];
  let quoted = false;
  let start = 0;
  for (let index = 0; index < projections.length; index += 1) {
    if (projections[index] === '"') {
      if (quoted && projections[index + 1] === '"') {
        index += 1;
        continue;
      }
      quoted = !quoted;
    } else if (!quoted && projections[index] === ",") {
      columns.push(projections.slice(start, index).trim());
      start = index + 1;
    }
  }
  columns.push(projections.slice(start).trim());
  return columns;
}

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}
