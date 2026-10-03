import { createSignal } from "solid-js";
import {
  compositionApi,
  type CompositionListItem,
  type CompositionParameterDefinition,
  type CompositionRawRevision,
  type CompositionResolvedSource,
  type CompositionResolveResponse,
  type CompositionSourcePage,
} from "./composition-api";

export interface CompositionIdentity {
  spaceId: string;
  compositionId: string;
  revisionId: string;
}

export interface CompositionSourcePageState {
  status: "loading" | "ready" | "error";
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

const abortError = (error: unknown): boolean =>
  !!error && typeof error === "object" &&
  (error as { name?: unknown }).name === "AbortError";

const defaultParameters = (
  definitions: readonly CompositionParameterDefinition[] | undefined,
): Record<string, unknown> => {
  const values: Record<string, unknown> = {};
  for (const definition of definitions ?? []) {
    if (definition.default !== undefined) {
      values[definition.id] = definition.default;
    }
  }
  return values;
};

const sourcePageNext = (page: CompositionSourcePage | undefined) => {
  if (!page) return undefined;
  return page.page.next;
};

/** Client-held Composition Work. It owns no saved state or query authority. */
export function createCompositionQueryHandle(
  api: CompositionQueryApi = compositionApi,
) {
  const [state, setState] = createSignal<CompositionQueryState>({
    opening: false,
    resolving: false,
    sources: {},
  });
  const [parameters, setParameters] = createSignal<Record<string, unknown>>(
    {},
  );

  let disposed = false;
  let generation = 0;
  let currentIdentityKey = "";
  let parametersInitialized = false;
  let openController: AbortController | undefined;
  let resolveController: AbortController | undefined;
  const sourceControllers = new Map<string, AbortController>();
  const sourceGenerations = new Map<string, number>();
  const resolvedSources = new Map<string, CompositionResolvedSource>();

  const isCurrent = (requestGeneration: number) =>
    !disposed && requestGeneration === generation;

  const abortSources = () => {
    for (const controller of sourceControllers.values()) controller.abort();
    sourceControllers.clear();
    sourceGenerations.clear();
    resolvedSources.clear();
  };

  const clearRequests = () => {
    openController?.abort();
    resolveController?.abort();
    openController = undefined;
    resolveController = undefined;
    abortSources();
  };

  const open = async (
    identity: CompositionIdentity,
    summary?: CompositionListItem,
  ) => {
    const key = JSON.stringify(identity);
    if (key === currentIdentityKey && !state().resolveError) return;
    currentIdentityKey = key;
    const requestGeneration = ++generation;
    clearRequests();
    setParameters({});
    parametersInitialized = false;
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
      await resolveCurrent(requestGeneration, identity, {});
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

  const resolveCurrent = async (
    requestGeneration: number,
    identity: CompositionIdentity,
    values: Record<string, unknown>,
  ) => {
    if (!isCurrent(requestGeneration)) return;
    resolveController?.abort();
    abortSources();
    setState((current) => ({
      ...current,
      resolving: true,
      resolveError: undefined,
      resolved: undefined,
      sources: {},
    }));
    const controller = new AbortController();
    resolveController = controller;
    try {
      const resolved = await api.resolve(
        identity.spaceId,
        identity.compositionId,
        identity.revisionId,
        values,
        controller.signal,
      );
      if (!isCurrent(requestGeneration) || controller.signal.aborted) return;
      if (!parametersInitialized) {
        setParameters({
          ...defaultParameters(resolved.parameter_definitions),
          ...values,
        });
        parametersInitialized = true;
      }
      setState((current) => ({
        ...current,
        resolving: false,
        resolved,
        sources: Object.fromEntries(
          (resolved.ok ? resolved.plan?.sources ?? [] : []).map((source) => [
            source.source_id,
            { status: "loading", cursorStack: [undefined] },
          ]),
        ),
      }));
      if (resolved.ok && resolved.plan) {
        await Promise.all(
          resolved.plan.sources.map((source) =>
            requestSource(
              requestGeneration,
              identity.spaceId,
              source,
              undefined,
              [undefined],
            )
          ),
        );
      }
    } catch (error) {
      if (isCurrent(requestGeneration) && !controller.signal.aborted) {
        setState((current) => ({
          ...current,
          resolving: false,
          resolved: undefined,
          sources: {},
          resolveError: error,
        }));
      }
    } finally {
      if (resolveController === controller) resolveController = undefined;
    }
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

  const setParameter = (parameterId: string, value: unknown | undefined) => {
    const next = { ...parameters() };
    if (value === undefined) delete next[parameterId];
    else next[parameterId] = value;
    setParameters(next);
    const current = state();
    if (!current.identity || !current.composition) return;
    const requestGeneration = ++generation;
    clearRequests();
    void resolveCurrent(requestGeneration, current.identity, next);
  };

  const next = (sourceId: string) => {
    const current = state().sources[sourceId];
    const source = resolvedSources.get(sourceId);
    const cursor = sourcePageNext(current?.page);
    if (!current || !source || !cursor || current.status === "loading") return;
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
    clearRequests();
  };

  return {
    state,
    parameters,
    open,
    setParameter,
    next,
    previous,
    retry,
    dispose,
  };
}
