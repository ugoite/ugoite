import { createSignal } from "solid-js";
import {
  compositionApi,
  type CompositionListItem,
  type CompositionVariableDefinition,
  type CompositionRawRevision,
  type CompositionResolvePlan,
  type CompositionResolvedComponentBinding,
  type CompositionResolvedSource,
  type CompositionResolveResponse,
  type CompositionSourcePage,
} from "./composition-api";
import {
  parseCompositionVariableInputSync,
  prepareCompositionVariableInputParser,
} from "./ugoite-client/protocol";

export interface CompositionIdentity {
  spaceId: string;
  compositionId: string;
  revisionId: string;
}

export type CompositionVariableInputStatus = "valid" | "unset" | "invalid";

/** One shared, transient raw/typed value for every Input bound to a variable. */
export interface CompositionVariableInputState {
  raw: string;
  status: CompositionVariableInputStatus;
  value?: unknown;
}

export interface CompositionSourcePageState {
  status: "loading" | "ready" | "error" | "invalid";
  cursorStack: (string | undefined)[];
  cursor?: string;
  page?: CompositionSourcePage;
  error?: unknown;
}

export interface CompositionQueryState {
  identity?: CompositionIdentity;
  summary?: CompositionListItem;
  composition?: CompositionRawRevision;
  opening: boolean;
  resolving: boolean;
  resolved?: CompositionResolveResponse;
  resolveError?: unknown;
  sources: Record<string, CompositionSourcePageState>;
}

export interface CompositionQueryApi {
  get: typeof compositionApi.get;
  resolve: typeof compositionApi.resolve;
  querySource: typeof compositionApi.querySource;
}

const SOURCE_RESOLVE_DEBOUNCE_MS = 120;

const abortError = (error: unknown): boolean =>
  !!error && typeof error === "object" &&
  (error as { name?: unknown }).name === "AbortError";

const sourcePageNext = (page: CompositionSourcePage | undefined) => {
  if (!page) return undefined;
  return page.page.next;
};

const sourceNeedsPage = (
  source: CompositionResolvedSource,
  bindings: readonly CompositionResolvedComponentBinding[],
): boolean =>
  bindings.some((binding) =>
    binding.kind === "table" && binding.source_id === source.source_id
  );

const defaultInputStates = (
  definitions: readonly CompositionVariableDefinition[] | undefined,
): Record<string, CompositionVariableInputState> =>
  Object.fromEntries((definitions ?? []).map((definition) => {
    const value = definition.default;
    if (value === undefined) {
      return [definition.id, { raw: "", status: "unset" }];
    }
    return [definition.id, {
      raw: typeof value === "string" ? value : String(value),
      status: "valid",
      value,
    }];
  }));

const variableSnapshot = (
  inputs: Readonly<Record<string, CompositionVariableInputState>>,
): Record<string, unknown> =>
  Object.fromEntries(Object.entries(inputs).map(([id, input]) => [
    id,
    input.status === "valid" ? input.value : null,
  ]));

const invalidVariableIds = (
  definitions: readonly CompositionVariableDefinition[],
  inputs: Readonly<Record<string, CompositionVariableInputState>>,
): Set<string> => {
  const blocked = new Set<string>();
  for (const definition of definitions) {
    const input = inputs[definition.id];
    if (
      input?.status === "invalid" ||
      (definition.required && input?.status === "unset")
    ) blocked.add(definition.id);
  }
  return blocked;
};

const invalidRawVariableIds = (
  inputs: Readonly<Record<string, CompositionVariableInputState>>,
): Set<string> => new Set(
  Object.entries(inputs).flatMap(([id, input]) =>
    input.status === "invalid" ? [id] : []
  ),
);

const sourceVariableIds = (
  plan: CompositionResolvePlan | undefined,
  sourceId: string,
): readonly string[] => {
  const consumers = plan?.dependencies?.variable_consumers;
  if (!consumers) return [];
  return Object.entries(consumers).flatMap(([variableId, entry]) =>
    entry.source_ids?.includes(sourceId) ? [variableId] : []
  );
};

const sourceBlocked = (
  plan: CompositionResolvePlan | undefined,
  sourceId: string,
  blockedVariables: ReadonlySet<string>,
): boolean =>
  sourceVariableIds(plan, sourceId).some((variableId) =>
    blockedVariables.has(variableId)
  );

const hasCompleteDependencyIndex = (
  plan: CompositionResolvePlan,
  definitions: readonly CompositionVariableDefinition[],
): boolean => {
  if (definitions.length === 0) return true;
  const consumers = plan.dependencies?.variable_consumers;
  return !!consumers && definitions.every((definition) =>
    Object.hasOwn(consumers, definition.id)
  );
};

const sourceFingerprint = (source: CompositionResolvedSource): string =>
  JSON.stringify(source);

/** Client-held Composition Work. It owns no saved state or query authority. */
export function createCompositionQueryHandle(
  api: CompositionQueryApi = compositionApi,
) {
  const [state, setState] = createSignal<CompositionQueryState>({
    opening: false,
    resolving: false,
    sources: {},
  });
  const [variableInputs, setVariableInputs] = createSignal<
    Record<string, CompositionVariableInputState>
  >({});

  let disposed = false;
  let generation = 0;
  let resolutionGeneration = 0;
  let currentIdentityKey = "";
  let variableDefinitions: CompositionVariableDefinition[] = [];
  let openController: AbortController | undefined;
  let resolveController: AbortController | undefined;
  let resolveTimer: ReturnType<typeof setTimeout> | undefined;
  const pendingSourceIds = new Set<string>();
  let forceResolve = false;
  const sourceControllers = new Map<string, AbortController>();
  const sourceGenerations = new Map<string, number>();
  const resolvedSources = new Map<string, CompositionResolvedSource>();

  const isCurrent = (requestGeneration: number) =>
    !disposed && requestGeneration === generation;

  const cancelScheduledResolve = () => {
    if (resolveTimer !== undefined) clearTimeout(resolveTimer);
    resolveTimer = undefined;
  };

  const invalidateSourceRequest = (sourceId: string) => {
    sourceControllers.get(sourceId)?.abort();
    sourceControllers.delete(sourceId);
    sourceGenerations.set(sourceId, (sourceGenerations.get(sourceId) ?? 0) + 1);
  };

  const abortSources = () => {
    for (const controller of sourceControllers.values()) controller.abort();
    sourceControllers.clear();
    sourceGenerations.clear();
    resolvedSources.clear();
  };

  const clearRequests = () => {
    cancelScheduledResolve();
    openController?.abort();
    resolveController?.abort();
    openController = undefined;
    resolveController = undefined;
    abortSources();
  };

  const definitions = () => variableDefinitions;
  const inputStates = () => variableInputs();
  const variables = () => variableSnapshot(variableInputs());
  const blockedVariables = () => {
    const plan = state().resolved?.plan;
    if (plan && !hasCompleteDependencyIndex(plan, variableDefinitions)) {
      return new Set(variableDefinitions.map((definition) => definition.id));
    }
    return invalidVariableIds(variableDefinitions, variableInputs());
  };
  const invalidRawVariables = () => invalidRawVariableIds(variableInputs());

  const markSourcesInvalid = (sourceIds: Iterable<string>) => {
    const currentPlan = state().resolved?.plan;
    const ids = [...sourceIds];
    for (const sourceId of ids) invalidateSourceRequest(sourceId);
    setState((current) => {
      const sources = { ...current.sources };
      for (const sourceId of ids) {
        const source = currentPlan?.sources.find((candidate) =>
          candidate.source_id === sourceId
        );
        if (!source || !sourceNeedsPage(source, currentPlan?.component_bindings ?? [])) {
          continue;
        }
        sources[sourceId] = {
          status: "invalid",
          cursorStack: sources[sourceId]?.cursorStack ?? [undefined],
          cursor: undefined,
        };
      }
      return { ...current, sources };
    });
  };

  const markPendingSourcesInvalid = () => {
    if (pendingSourceIds.size > 0) markSourcesInvalid(pendingSourceIds);
  };

  const requestSource = async (
    requestGeneration: number,
    spaceId: string,
    source: CompositionResolvedSource,
    cursor: string | undefined,
    cursorStack: (string | undefined)[],
  ) => {
    if (!isCurrent(requestGeneration)) return;
    const sourceId = source.source_id;
    sourceControllers.get(sourceId)?.abort();
    const sourceGeneration = (sourceGenerations.get(sourceId) ?? 0) + 1;
    sourceGenerations.set(sourceId, sourceGeneration);
    resolvedSources.set(sourceId, source);
    const controller = new AbortController();
    sourceControllers.set(sourceId, controller);
    setState((current) => ({
      ...current,
      sources: {
        ...current.sources,
        [sourceId]: {
          status: "loading",
          cursorStack,
          ...(cursor === undefined ? {} : { cursor }),
        },
      },
    }));
    try {
      const page = await api.querySource(
        spaceId,
        source,
        cursor,
        controller.signal,
      );
      if (
        !isCurrent(requestGeneration) || controller.signal.aborted ||
        sourceGenerations.get(sourceId) !== sourceGeneration
      ) return;
      setState((current) => ({
        ...current,
        sources: {
          ...current.sources,
          [sourceId]: { status: "ready", cursorStack, cursor, page },
        },
      }));
    } catch (error) {
      if (
        isCurrent(requestGeneration) && !controller.signal.aborted &&
        !abortError(error) &&
        sourceGenerations.get(sourceId) === sourceGeneration
      ) {
        setState((current) => ({
          ...current,
          sources: {
            ...current.sources,
            [sourceId]: { status: "error", cursorStack, cursor, error },
          },
        }));
      }
    } finally {
      if (sourceControllers.get(sourceId) === controller) {
        sourceControllers.delete(sourceId);
      }
    }
  };

  const resolveCurrent = async (
    requestGeneration: number,
    identity: CompositionIdentity,
    values: Record<string, unknown>,
    sourcesChanged: ReadonlySet<string>,
    requestResolutionGeneration: number,
  ) => {
    if (!isCurrent(requestGeneration)) return;
    resolveController?.abort();
    const controller = new AbortController();
    resolveController = controller;
    setState((current) => ({ ...current, resolving: true }));
    try {
      const response = await api.resolve(
        identity.spaceId,
        identity.compositionId,
        identity.revisionId,
        values,
        controller.signal,
      );
      if (
        !isCurrent(requestGeneration) || controller.signal.aborted ||
        requestResolutionGeneration !== resolutionGeneration
      ) return;

      if (response.variable_definitions) {
        variableDefinitions = response.variable_definitions;
        if (Object.keys(variableInputs()).length === 0) {
          setVariableInputs(defaultInputStates(variableDefinitions));
        }
      }

      const current = state();
      const previousPlan = current.resolved?.plan;
      if (!response.ok || !response.plan) {
        const blocked = blockedVariables();
        const plan = previousPlan;
        const sourceIds = new Set(sourcesChanged);
        if (!plan) {
          for (const source of response.plan?.sources ?? []) {
            sourceIds.add(source.source_id);
          }
        }
        const nextSources = { ...current.sources };
        for (const sourceId of sourceIds) {
          const source = plan?.sources.find((candidate) =>
            candidate.source_id === sourceId
          );
          if (!source || !sourceNeedsPage(source, plan?.component_bindings ?? [])) {
            continue;
          }
          nextSources[sourceId] = {
            ...(nextSources[sourceId] ?? { cursorStack: [undefined] }),
            status: sourceBlocked(plan, sourceId, blocked) ? "invalid" : "error",
            cursorStack: [undefined],
            cursor: undefined,
          };
        }
        setState((value) => ({
          ...value,
          resolving: false,
          resolved: { ...response, ...(plan ? { plan } : {}) },
          sources: nextSources,
          ...(!plan && response.diagnostics?.length === 0
            ? { resolveError: new Error("Composition could not be resolved") }
            : {}),
        }));
        return;
      }

      const nextPlan = response.plan;
      if (!hasCompleteDependencyIndex(nextPlan, variableDefinitions)) {
        const invalidSources: Record<string, CompositionSourcePageState> = {};
        const knownSources = new Set(nextPlan.sources.map((source) => source.source_id));
        const invalidated = new Set<string>();
        for (const sourceId of Object.keys(current.sources)) {
          invalidateSourceRequest(sourceId);
          invalidated.add(sourceId);
        }
        for (const sourceId of resolvedSources.keys()) {
          if (!knownSources.has(sourceId) && !invalidated.has(sourceId)) {
            invalidateSourceRequest(sourceId);
          }
        }
        resolvedSources.clear();
        for (const source of nextPlan.sources) {
          if (sourceNeedsPage(source, nextPlan.component_bindings)) {
            invalidSources[source.source_id] = {
              status: "invalid",
              cursorStack: [undefined],
            };
          }
        }
        setState((value) => ({
          ...value,
          resolving: false,
          resolved: response,
          sources: invalidSources,
          resolveError: new Error("Composition dependency index is incomplete"),
        }));
        return;
      }
      const previousById = new Map(
        (previousPlan?.sources ?? []).map((source) => [source.source_id, source]),
      );
      const nextById = new Map(
        nextPlan.sources.map((source) => [source.source_id, source]),
      );
      const nextSources: Record<string, CompositionSourcePageState> = {};
      const pagesToLoad: CompositionResolvedSource[] = [];
      resolvedSources.clear();
      for (const source of nextPlan.sources) {
        resolvedSources.set(source.source_id, source);
        if (!sourceNeedsPage(source, nextPlan.component_bindings)) continue;
        const previousSource = previousById.get(source.source_id);
        const previousState = current.sources[source.source_id];
        const affected = sourcesChanged.has(source.source_id) || !previousSource;
        const sameRequest = previousSource !== undefined &&
          sourceFingerprint(previousSource) === sourceFingerprint(source);
        if (
          sameRequest && previousState?.page !== undefined &&
          (previousState.status === "ready" || affected)
        ) {
          nextSources[source.source_id] = previousState.status === "ready"
            ? previousState
            : { ...previousState, status: "ready" };
          continue;
        }
        // A re-resolution for another variable must not restart this source
        // while its later page is in flight. Preserve all unchanged requests,
        // including loading and error states; only changed dependencies or an
        // explicit force-resolve may start a first-page query again.
        if (!affected && sameRequest && previousState) {
          nextSources[source.source_id] = previousState;
          continue;
        }
        nextSources[source.source_id] = {
          status: "loading",
          cursorStack: [undefined],
        };
        pagesToLoad.push(source);
      }
      // Remove pages for sources no longer present in the authoritative plan.
      for (const sourceId of Object.keys(current.sources)) {
        if (!nextById.has(sourceId)) invalidateSourceRequest(sourceId);
      }
      setState((value) => ({
        ...value,
        resolving: false,
        resolved: response,
        sources: nextSources,
        resolveError: undefined,
      }));
      await Promise.all(pagesToLoad.map((source) =>
        requestSource(
          requestGeneration,
          identity.spaceId,
          source,
          undefined,
          [undefined],
        )
      ));
    } catch (error) {
      if (
        isCurrent(requestGeneration) && !controller.signal.aborted &&
        requestResolutionGeneration === resolutionGeneration
      ) {
        const current = state();
        if (!current.resolved?.plan) {
          setState((value) => ({
            ...value,
            resolving: false,
            resolveError: error,
          }));
        } else {
          const sources = { ...current.sources };
          for (const sourceId of sourcesChanged) {
            const source = current.resolved.plan.sources.find((candidate) =>
              candidate.source_id === sourceId
            );
            if (!source || !sourceNeedsPage(source, current.resolved.plan.component_bindings)) continue;
            sources[sourceId] = {
              ...(sources[sourceId] ?? { cursorStack: [undefined] }),
              status: "error",
              cursorStack: [undefined],
              cursor: undefined,
              error,
            };
          }
          setState((value) => ({ ...value, resolving: false, sources }));
        }
      }
    } finally {
      if (resolveController === controller) resolveController = undefined;
    }
  };

  const resolveScheduled = () => {
    cancelScheduledResolve();
    if (invalidRawVariables().size > 0) {
      markPendingSourcesInvalid();
      return;
    }
    if (pendingSourceIds.size === 0 && !forceResolve) return;
    const current = state();
    if (!current.identity || !current.composition) return;
    const changed = new Set(pendingSourceIds);
    const force = forceResolve;
    pendingSourceIds.clear();
    forceResolve = false;
    const requestResolutionGeneration = ++resolutionGeneration;
    resolveController?.abort();
    void resolveCurrent(
      generation,
      current.identity,
      variables(),
      changed,
      requestResolutionGeneration,
    );
  };

  const scheduleResolve = () => {
    if (invalidRawVariables().size > 0) {
      markPendingSourcesInvalid();
      return;
    }
    cancelScheduledResolve();
    resolveTimer = setTimeout(resolveScheduled, SOURCE_RESOLVE_DEBOUNCE_MS);
  };

  const open = async (
    identity: CompositionIdentity,
    summary?: CompositionListItem,
  ) => {
    const key = JSON.stringify(identity);
    if (key === currentIdentityKey && !state().resolveError) return;
    currentIdentityKey = key;
    const requestGeneration = ++generation;
    resolutionGeneration += 1;
    clearRequests();
    pendingSourceIds.clear();
    forceResolve = false;
    variableDefinitions = [];
    setVariableInputs({});
    setState({
      identity,
      summary,
      opening: true,
      resolving: false,
      sources: {},
    });

    const controller = new AbortController();
    openController = controller;
    try {
      // Load the canonical Rust parser before any Input control is rendered.
      await prepareCompositionVariableInputParser();
      const composition = await api.get(
        identity.spaceId,
        identity.compositionId,
        identity.revisionId,
        controller.signal,
      );
      if (!isCurrent(requestGeneration)) return;
      if (
        composition.revision.entry_id !== identity.compositionId ||
        composition.revision.revision_id !== identity.revisionId
      ) {
        throw new Error("Composition revision mismatch");
      }
      setState((current) => ({ ...current, composition, opening: false }));
      const initialResolutionGeneration = ++resolutionGeneration;
      await resolveCurrent(
        requestGeneration,
        identity,
        {},
        new Set(),
        initialResolutionGeneration,
      );
    } catch (error) {
      if (isCurrent(requestGeneration) && !controller.signal.aborted) {
        setState((current) => ({
          ...current,
          opening: false,
          resolving: false,
          resolveError: error,
        }));
      }
    } finally {
      if (openController === controller) openController = undefined;
    }
  };

  const setVariableRaw = (variableId: string, raw: string) => {
    const definition = variableDefinitions.find((item) => item.id === variableId);
    if (!definition) return;
    const parsed = parseCompositionVariableInputSync(definition.type, raw);
    const previous = variableInputs()[variableId];
    const next: CompositionVariableInputState = parsed.status === "valid"
      ? { raw, status: "valid", value: parsed.value }
      : parsed.status === "unset"
      ? { raw, status: "unset" }
      : { raw, status: "invalid" };
    setVariableInputs((current) => ({ ...current, [variableId]: next }));

    const semanticallyEqual = previous?.status === next.status &&
      (next.status !== "valid" || Object.is(previous?.value, next.value));
    if (semanticallyEqual) return;

    // Prevent an earlier variable snapshot from resolving during the short
    // debounce window. Text and shared Inputs already reflect the new store.
    resolutionGeneration += 1;
    resolveController?.abort();

    const current = state();
    const plan = current.resolved?.plan;
    if (plan && !hasCompleteDependencyIndex(plan, variableDefinitions)) {
      markSourcesInvalid(plan.sources.map((source) => source.source_id));
      setState((value) => ({
        ...value,
        resolving: false,
        resolveError: new Error("Composition dependency index is incomplete"),
      }));
      resolutionGeneration += 1;
      resolveController?.abort();
      cancelScheduledResolve();
      return;
    }
    const sourceIds = plan?.dependencies?.variable_consumers?.[variableId]
      ?.source_ids ?? [];
    const blocked = next.status === "invalid" ||
      (definition.required && next.status === "unset");
    for (const sourceId of sourceIds) {
      invalidateSourceRequest(sourceId);
      setState((value) => {
        const source = plan?.sources.find((item) => item.source_id === sourceId);
        if (!source || !sourceNeedsPage(source, plan?.component_bindings ?? [])) {
          return value;
        }
        const previousSource = value.sources[sourceId];
        return {
          ...value,
          sources: {
            ...value.sources,
            [sourceId]: {
              status: blocked ? "invalid" : "loading",
              cursorStack: blocked ? previousSource?.cursorStack ?? [undefined] : [undefined],
              cursor: undefined,
            },
          },
        };
      });
      if (next.status !== "invalid") pendingSourceIds.add(sourceId);
    }
    // Resolve the typed snapshot for diagnostics and authoritative planning,
    // but only dependency-indexed Sources may trigger a new page request.
    forceResolve = true;
    if (next.status === "invalid") {
      resolutionGeneration += 1;
      resolveController?.abort();
      cancelScheduledResolve();
      markPendingSourcesInvalid();
      return;
    }
    scheduleResolve();
  };

  const next = (sourceId: string) => {
    const current = state().sources[sourceId];
    const source = resolvedSources.get(sourceId);
    const cursor = sourcePageNext(current?.page);
    if (!current || !source || !cursor || current.status !== "ready") return;
    void requestSource(
      generation,
      state().identity!.spaceId,
      source,
      cursor,
      [...current.cursorStack, cursor],
    );
  };

  const previous = (sourceId: string) => {
    const current = state().sources[sourceId];
    const source = resolvedSources.get(sourceId);
    if (!current || !source || current.cursorStack.length <= 1) return;
    const cursorStack = current.cursorStack.slice(0, -1);
    void requestSource(
      generation,
      state().identity!.spaceId,
      source,
      cursorStack.at(-1),
      cursorStack,
    );
  };

  const retry = (sourceId: string) => {
    const current = state().sources[sourceId];
    if (current?.status === "invalid") return;
    const source = resolvedSources.get(sourceId);
    if (!current || !source || current.status === "loading") return;
    void requestSource(
      generation,
      state().identity!.spaceId,
      source,
      current.cursor,
      current.cursorStack,
    );
  };

  const dispose = () => {
    disposed = true;
    generation += 1;
    resolutionGeneration += 1;
    clearRequests();
  };

  return {
    state,
    variables,
    variableInputs: inputStates,
    invalidVariables: blockedVariables,
    open,
    setVariableRaw,
    next,
    previous,
    retry,
    dispose,
  };
}
