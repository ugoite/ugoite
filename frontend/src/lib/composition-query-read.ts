import type { CompositionEvaluationResponse } from "./composition-api";
import type { CompositionGraphDraft } from "./composition-variable-binding";
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
    descriptor?.returns.kind !== "table" ||
    !evaluation.query_results[variableId]?.count_snapshot
  ) {
    throw new Error("A current validated table query is required");
  }
  if (node.query.kind !== "inline_sql") {
    throw new Error("A current validated table query is required");
  }
  const sql = node.query.sql;
  const form_binding_guards = (descriptor.form_bindings ?? [])
    .filter(({ relation, form_id }) =>
      relation !== `form_${form_id.replaceAll("-", "").toLowerCase()}`
    )
    .map(({ relation, form_id }) => ({ name: relation, form_id }));
  // Use the exact graph SQL from evaluation. The same resolved identity guards
  // bind human Form names on both requests; opaque Form ID relations stay raw.
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
    const value: unknown = result?.kind === "scalar"
      ? result.value
      : evaluation.variable_values[argument.variable];
    if (
      !valueType || value === undefined ||
      valueType.scalar_type === "decimal" ||
      valueType.scalar_type === "json"
    ) {
      throw new Error("A validated SQL scalar argument is required");
    }
    const taggedLiteral = value !== null && typeof value === "object" &&
      "kind" in value;
    if (taggedLiteral) {
      if (value.kind === "null") {
        parameters[argument.name] = null;
      } else if ("value" in value) {
        parameters[argument.name] = value.value;
      } else {
        throw new Error("A validated SQL scalar argument is required");
      }
    } else {
      parameters[argument.name] = value;
    }
    parameter_types[argument.name] = valueType.scalar_type;
  }
  return {
    // Inline queries use the resolver's stable Form IDs, never
    // current-name rebinding.
    sql,
    composition_count_snapshot: evaluation.query_results[variableId]!
      .count_snapshot,
    parameters,
    parameter_types,
    ...(form_binding_guards.length ? { form_binding_guards } : {}),
  };
}
