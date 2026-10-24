import type {
  SqlDocument,
  SqlDocumentIdentifier,
  SqlDocumentNode,
} from "~/lib/ugoite-client";

/** Request-scoped authorized schema; the owner guards SQL, parameters and host identity. */
export interface PlannedSqlColumns {
  columns: readonly string[];
  isCurrent: () => boolean | Promise<boolean>;
}
export type PlanSqlColumns = () => Promise<PlannedSqlColumns>;

/** Eligibility only. Form metadata cannot establish wildcard output names or order. */
export function wildcardProjectionQualifier(
  document: SqlDocument,
  item: SqlDocumentNode,
  knownRelations: readonly string[],
): SqlDocumentIdentifier | undefined {
  if (
    document.status !== "parsed" || item.semantic?.kind !== "wildcard" ||
    item.semantic.has_options || item.semantic.qualifier_is_expression ||
    item.semantic.qualifier.length > 1
  ) return;
  const nodes = new Map(document.nodes.map((node) => [node.id, node]));
  const selects = document.nodes.filter((node) => node.kind === "select");
  if (selects.length !== 1) return;
  const select = selects[0];
  const query = select.parent === null ? undefined : nodes.get(select.parent);
  const statement = query?.parent === null
    ? undefined
    : nodes.get(query?.parent ?? -1);
  if (query?.kind !== "query" || statement?.kind !== "query_statement") return;
  const list = item.parent === null ? undefined : nodes.get(item.parent);
  if (
    list?.kind !== "select_list" || list.parent !== select.id ||
    document.nodes.filter((node) =>
        node.kind === "select_item" && node.parent === list.id
      ).length !== 1
  ) return;
  const factors = document.nodes.filter((node) => node.kind === "table_factor");
  const known = factors.flatMap((factor) => {
    if (
      factor.semantic?.kind !== "table_factor" ||
      factor.semantic.factor_kind !== "table"
    ) return [];
    const relation = document.nodes.find((node) =>
      node.parent === factor.id && node.semantic?.kind === "relation"
    );
    if (
      relation?.semantic?.kind !== "relation" ||
      relation.semantic.name.length !== 1 ||
      !knownRelations.includes(relation.semantic.name[0].value)
    ) return [];
    return [factor.semantic.alias ?? relation.semantic.name[0]];
  });
  const qualifier = item.semantic.qualifier.at(-1)?.value;
  if (!qualifier) {
    return factors.length === 1 && known.length === 1 ? known[0] : undefined;
  }
  const selected = known.filter((candidate) => candidate.value === qualifier);
  return selected.length === 1 ? selected[0] : undefined;
}

export function usablePlannedColumnNames(columns: readonly string[]): boolean {
  return columns.length > 0 && columns.length <= 256 &&
    new Set(columns).size === columns.length &&
    columns.every((name) => name.length > 0 && !name.includes("\0"));
}
