export type CompositionFormSeedField = {
  label?: string;
  sql_column?: string;
  query_capability?: { projectable?: boolean };
};

export function expectedCompositionFormSeedSql(
  relation: string,
  fields: Record<string, CompositionFormSeedField> | undefined,
): string {
  const aliases = new Set<string>();
  const columns = Object.entries(fields ?? {}).flatMap(([name, field]) => {
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
  });
  if (columns.length === 0) {
    throw new Error("Form seed expectations need a projectable field");
  }
  return `SELECT ${columns.join(", ")} FROM ${sqlIdentifier(relation)} ORDER BY "_ugoite_updated_at" DESC, "_ugoite_id"`;
}

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}
