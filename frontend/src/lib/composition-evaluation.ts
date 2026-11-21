import type {
  CompositionDependencyIndex,
  CompositionEvaluationResponse,
} from "~/lib/composition-api";
import type { CompositionGraphDraft } from "~/lib/composition-variable-binding";

export interface CompositionEvaluationIdentity {
  compositionRevision: string;
  publicationFingerprint: string;
  authorizationFingerprint: string;
}

/**
 * Walk query dependencies from changed variables. Adapters provide the graph
 * edges in their native shape; traversal and stable output ordering stay shared.
 */
export const compositionDependencyClosure = (
  queryIds: readonly string[],
  consumersByVariable: ReadonlyMap<string, readonly string[]>,
  changedVariableIds: ReadonlySet<string>,
): string[] => {
  const knownQueries = new Set(queryIds);
  const affected = new Set<string>();
  const pending = [...changedVariableIds];

  while (pending.length > 0) {
    const variableId = pending.pop()!;
    for (const queryId of consumersByVariable.get(variableId) ?? []) {
      if (!knownQueries.has(queryId) || affected.has(queryId)) continue;
      affected.add(queryId);
      pending.push(queryId);
    }
  }

  return queryIds.filter((queryId) => affected.has(queryId));
};

/**
 * Browser-owned scheduling hint for graph drafts, including temporarily invalid
 * drafts. Core remains authoritative for validating dependencies and execution.
 */
export const dependentCompositionQueryIds = (
  draft: CompositionGraphDraft,
  variableIds: ReadonlySet<string>,
): string[] => {
  const queries = draft.variables.filter((node) => node.kind === "query");
  const consumersByVariable = new Map<string, string[]>();
  for (const query of queries) {
    for (const argument of query.query.arguments) {
      const consumers = consumersByVariable.get(argument.variable) ?? [];
      consumers.push(query.id);
      consumersByVariable.set(argument.variable, consumers);
    }
  }
  return compositionDependencyClosure(
    queries.map((query) => query.id),
    consumersByVariable,
    variableIds,
  );
};

/** Scheduling hints only; Core independently validates the selected dependency closure. */
export const unavailableCompositionQueryIds = (
  draft: CompositionGraphDraft,
  states: Readonly<Record<string, "pending" | "invalid" | "missing">>,
  supplied: Readonly<Record<string, unknown>>,
  unset: ReadonlySet<string>,
): string[] => {
  const unavailable = new Set(Object.keys(states));
  for (const node of draft.variables) {
    if (
      node.kind === "input" && node.required &&
      (unset.has(node.id) ||
        (!Object.hasOwn(supplied, node.id) && node.default === undefined))
    ) unavailable.add(node.id);
  }
  return dependentCompositionQueryIds(draft, unavailable);
};

export const changedCompositionQueryIds = (
  previous: CompositionGraphDraft,
  next: CompositionGraphDraft,
): string[] => {
  const previousById = new Map(
    previous.variables.map((node) => [node.id, node]),
  );
  const nextById = new Map(next.variables.map((node) => [node.id, node]));
  const variableIds = new Set([...previousById.keys(), ...nextById.keys()]);
  const changed = new Set(
    [...variableIds].filter((id) =>
      JSON.stringify(previousById.get(id)) !== JSON.stringify(nextById.get(id))
    ),
  );
  const affected = new Set<string>([
    ...[...changed].filter((id) =>
      previousById.get(id)?.kind === "query" ||
      nextById.get(id)?.kind === "query"
    ),
    ...dependentCompositionQueryIds(previous, changed),
    ...dependentCompositionQueryIds(next, changed),
  ]);
  return [...affected];
};

/**
 * These three server-owned values define whether a browser-held query result
 * can be reused. Input and query fingerprints intentionally vary per edit.
 */
export const compositionEvaluationIdentity = (
  response: CompositionEvaluationResponse | undefined,
): CompositionEvaluationIdentity | undefined => {
  const revision = response?.composition_revision;
  const publicationFingerprint = response?.publication_fingerprint;
  const authorizationFingerprint = response?.authorization_fingerprint;
  if (
    !revision?.entry_id || !revision.revision_id ||
    !publicationFingerprint || !authorizationFingerprint
  ) return undefined;
  return {
    compositionRevision: `${revision.entry_id}/${revision.revision_id}`,
    publicationFingerprint,
    authorizationFingerprint,
  };
};

export const sameCompositionEvaluationIdentity = (
  left: CompositionEvaluationIdentity | undefined,
  right: CompositionEvaluationIdentity | undefined,
): boolean =>
  Boolean(
    left && right &&
      left.compositionRevision === right.compositionRevision &&
      left.publicationFingerprint === right.publicationFingerprint &&
      left.authorizationFingerprint === right.authorizationFingerprint,
  );

/** Find every query whose direct or transitive argument graph uses a change. */
export const affectedCompositionQueries = (
  queries: CompositionEvaluationResponse["queries"],
  dependencies: CompositionDependencyIndex,
  changedVariableIds: ReadonlySet<string>,
): string[] => {
  const queryIds = queries.map((query) => query.variable_id);
  const consumersByVariable = new Map<string, string[]>();
  const variableConsumers = dependencies.variable_consumers ?? {};
  for (const [variableId, consumers] of Object.entries(variableConsumers)) {
    for (const queryId of consumers.query_variable_ids ?? []) {
      const consumersForVariable = consumersByVariable.get(variableId) ?? [];
      consumersForVariable.push(queryId);
      consumersByVariable.set(variableId, consumersForVariable);
    }
  }
  // Input/constant edges are represented by variable_consumers. Query-to-
  // query edges are represented by planner-resolved deferred arguments.
  for (const query of queries) {
    for (const argument of query.deferred_arguments) {
      const consumers = consumersByVariable.get(argument.variable_id) ?? [];
      consumers.push(query.variable_id);
      consumersByVariable.set(argument.variable_id, consumers);
    }
  }
  return compositionDependencyClosure(
    queryIds,
    consumersByVariable,
    changedVariableIds,
  );
};

/**
 * Merge a partial evaluation only when its authority snapshot is unchanged.
 * The caller must send a full evaluation after an identity change.
 */
export const mergeCompositionEvaluation = (
  previous: CompositionEvaluationResponse | undefined,
  next: CompositionEvaluationResponse,
  invalidatedQueryIds: readonly string[] = [],
): CompositionEvaluationResponse => {
  if (
    !previous ||
    !sameCompositionEvaluationIdentity(
      compositionEvaluationIdentity(previous),
      compositionEvaluationIdentity(next),
    )
  ) return next;

  const invalidated = new Set([
    ...invalidatedQueryIds,
    ...next.computed_query_ids,
  ]);
  const queryResults = { ...previous.query_results };
  for (const id of invalidated) delete queryResults[id];
  Object.assign(queryResults, next.query_results);

  const queryDiagnostics = [
    ...(previous.query_diagnostics ?? []).filter((diagnostic) =>
      !invalidated.has(diagnostic.variable_id)
    ),
    ...(next.query_diagnostics ?? []),
  ];
  // Partial evaluations only return metadata for the queries selected by the
  // caller. Preserve descriptors for the rest of this graph so a later input
  // edit can still discover its affected queries. A same-ID descriptor from
  // the latest response wins in case planner metadata has been refreshed.
  const queriesById = new Map(
    previous.queries.map((query) => [query.variable_id, query]),
  );
  for (const query of next.queries) {
    queriesById.set(query.variable_id, query);
  }
  return {
    ...next,
    queries: [...queriesById.values()],
    query_results: queryResults,
    query_diagnostics: queryDiagnostics,
  };
};
