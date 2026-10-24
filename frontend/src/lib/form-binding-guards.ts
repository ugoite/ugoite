import { parseSqlDocument } from "./ugoite-client/protocol";
import type { SqlFormBinding } from "./types";

/** Table references classified by Rust's SQL document parser. */
export async function sqlDocumentRelationNames(
  sql: string,
): Promise<ReadonlySet<string> | undefined> {
  const document = await parseSqlDocument(sql);
  if (document.status !== "parsed" && document.status !== "empty") {
    return undefined;
  }
  return new Set(document.nodes.flatMap((node) => {
    if (node.kind !== "table_factor") return [];
    const relation = document.nodes.find((child) =>
      child.parent === node.id && child.semantic?.kind === "relation"
    );
    if (relation?.semantic?.kind !== "relation") return [];
    const name = relation.semantic.name.map((part) => part.value).join(".");
    return name ? [name] : [];
  }));
}

/** Keep only guards still referenced by a valid Rust-parsed SQL document. */
export async function retainReferencedSqlFormBindingGuards(
  sql: string,
  guards: readonly SqlFormBinding[],
): Promise<SqlFormBinding[]> {
  const names = await sqlDocumentRelationNames(sql);
  return names ? guards.filter((guard) => names.has(guard.name)) : [...guards];
}
