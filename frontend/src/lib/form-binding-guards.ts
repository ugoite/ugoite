import { collectSqlFormRelationNames } from "./ugoite-client/protocol";
import type { SqlFormBinding } from "./types";

/** Table references classified by Rust's SQL document parser. */
export async function sqlDocumentRelationNames(
  sql: string,
): Promise<ReadonlySet<string> | undefined> {
  return new Set(await collectSqlFormRelationNames(sql));
}

/** Keep only guards still referenced by a valid Rust-parsed SQL document. */
export async function retainReferencedSqlFormBindingGuards(
  sql: string,
  guards: readonly SqlFormBinding[],
): Promise<SqlFormBinding[]> {
  try {
    const names = await sqlDocumentRelationNames(sql);
    return names
      ? guards.filter((guard) => names.has(guard.name))
      : [...guards];
  } catch {
    return [...guards];
  }
}

/** Atomically replace one picker identity, then prune guards no longer referenced. */
export async function replaceSqlFormBindingGuard(
  sql: string,
  guards: readonly SqlFormBinding[],
  selected: SqlFormBinding,
): Promise<SqlFormBinding[]> {
  return await retainReferencedSqlFormBindingGuards(sql, [
    ...guards.filter((guard) => guard.name !== selected.name),
    selected,
  ]);
}
