import type { CompositionEvaluationResponse } from "./composition-api";
import type {
  CompositionGraphDraft,
  CompositionLiteral,
} from "./composition-variable-binding";
import type { SqlQueryCountRequest } from "./types";

/** Project validated graph values into the existing explicit SQL count API. */
export function compositionQueryCountRequest(
  draft: CompositionGraphDraft,
  evaluation: CompositionEvaluationResponse,
  variableId: string,
): SqlQueryCountRequest {
  const node = draft.variables.find((entry) => entry.id === variableId);
  const descriptor = evaluation.queries.find((entry) =>
    entry.variable_id === variableId
  );
  if (
    node?.kind !== "query" || node.returns.kind !== "table" ||
    descriptor?.returns.kind !== "table" || !descriptor.portable_sql ||
    !evaluation.query_results[variableId]
  ) {
    throw new Error("A current validated table query is required");
  }
  const parameters: Record<string, unknown> = Object.create(null);
  const parameter_types: Record<string, string> = Object.create(null);
  for (const argument of node.query.arguments) {
    const variable = draft.variables.find((entry) =>
      entry.id === argument.variable
    );
    const valueType = variable?.kind === "query"
      ? variable.returns.kind === "scalar"
        ? variable.returns.value_type
        : undefined
      : variable?.value_type;
    const result = evaluation.query_results[argument.variable]?.value;
    const literal: CompositionLiteral | undefined = result?.kind === "scalar"
      ? result.value
      : evaluation.variable_values[argument.variable];
    if (
      !valueType || !literal || valueType.scalar_type === "decimal" ||
      valueType.scalar_type === "json"
    ) {
      throw new Error("A validated SQL scalar argument is required");
    }
    parameters[argument.name] = literal.kind === "null" ? null : literal.value;
    parameter_types[argument.name] = valueType.scalar_type;
  }
  return {
    // Exact Saved SQL is resolved again by the server. Inline queries use
    // the resolver's stable Form IDs, never current-name rebinding.
    sql: node.query.kind === "saved_sql" ? "" : descriptor.portable_sql,
    ...(node.query.kind === "saved_sql"
      ? {
        saved_sql: {
          id: node.query.entry_id,
          revision_id: node.query.revision_id,
        },
      }
      : {}),
    parameters,
    parameter_types,
  };
}
