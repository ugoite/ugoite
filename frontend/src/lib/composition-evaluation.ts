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

/** Return query values invalidated by a semantic graph-draft change. */
export const dependentCompositionQueryIds = (
  draft: CompositionGraphDraft,
  variableIds: ReadonlySet<string>,
): string[] => {
  const queries = draft.variables.filter((node) => node.kind === "query");
  const affected = new Set<string>();
  for (const variableId of variableIds) {
    for (const query of queries) {
      if (
        query.query.arguments.some((argument) =>
          argument.variable === variableId
        )
      ) affected.add(query.id);
    }
  }

  let found = true;
  while (found) {
    found = false;
    for (const query of queries) {
      if (affected.has(query.id)) continue;
      if (
        query.query.arguments.some((argument) =>
          affected.has(argument.variable)
        )
      ) {
        affected.add(query.id);
        found = true;
      }
    }
  }
  return [...affected];
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
  const queryIds = new Set(queries.map((query) => query.variable_id));
  const affected = new Set<string>();
  const variableConsumers = dependencies.variable_consumers ?? {};
  for (const variableId of changedVariableIds) {
    for (
      const queryId of variableConsumers[variableId]?.query_variable_ids ?? []
    ) {
      if (queryIds.has(queryId)) affected.add(queryId);
    }
  }

  // Input/constant edges are represented by variable_consumers. Query-to-
  // query edges are represented by planner-resolved deferred arguments.
  let found = true;
  while (found) {
    found = false;
    for (const query of queries) {
      if (affected.has(query.variable_id)) continue;
      if (
        query.deferred_arguments.some((argument) =>
          affected.has(argument.variable_id)
        )
      ) {
        affected.add(query.variable_id);
        found = true;
      }
    }
  }
  return queries.flatMap((query) =>
    affected.has(query.variable_id) ? [query.variable_id] : []
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
