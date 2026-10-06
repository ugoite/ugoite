import { createSignal } from "solid-js";
import type {
  CompositionPreviewResponse,
  CompositionResolvedSource,
  CompositionSourcePage,
} from "./composition-api";
import { compositionApi } from "./composition-api";

/** Execution identity of one draft preview: the draft fingerprint. */
export interface CompositionPreviewIdentity {
  spaceId: string;
  fingerprint: string;
}

export interface CompositionResolvedSourcePageState {
  status: "loading" | "ready" | "error";
  cursorStack: (string | undefined)[];
  cursor?: string;
  page?: CompositionSourcePage;
  error?: unknown;
}

export interface CompositionPreviewHandleState {
  /** Last requested candidate YAML; kept as Work for parameter re-preview. */
  yaml?: string;
  spaceId?: string;
  identity?: CompositionPreviewIdentity;
  previewing: boolean;
  preview?: CompositionPreviewResponse;
  previewError?: unknown;
  sources: Record<string, CompositionResolvedSourcePageState>;
}

/**
 * Bounded fetch scope for one draft preview. Page execution covers the
 * visible-component sources plus the Data-selected source only; every plan
 * source stays registered for controller-owned paging, so continuation and
 * retry keep the same ACL-checked path. Omitted scope fetches every plan
 * source, preserving the previous behavior for existing callers.
 */
export interface CompositionPreviewScope {
  visibleSourceIds?: readonly string[];
  selectedSourceId?: string | null;
}

export interface CompositionPreviewQueryApi {
  preview: (
    spaceId: string,
    yaml: string,
    parameters: Record<string, unknown>,
    signal?: AbortSignal,
  ) => Promise<CompositionPreviewResponse>;
  querySource: (
    spaceId: string,
    source: CompositionResolvedSource,
    cursor: string | undefined,
    signal: AbortSignal,
  ) => Promise<CompositionSourcePage>;
}

const abortError = (error: unknown): boolean =>
  !!error && typeof error === "object" &&
  (error as { name?: unknown }).name === "AbortError";

const sourcePageNext = (page: CompositionSourcePage | undefined) =>
  page?.page.next;

/** Scoped fetch set: visible-component sources plus the selected source. */
const scopedSourceIds = (
  sources: readonly CompositionResolvedSource[],
  scope: CompositionPreviewScope | undefined,
): Set<string> => {
  if (scope === undefined) {
    return new Set(sources.map((source) => source.source_id));
  }
  const wanted = new Set(scope.visibleSourceIds ?? []);
  if (scope.selectedSourceId != null) wanted.add(scope.selectedSourceId);
  return wanted;
};

/**
 * Client-held draft preview Work. Execution identity is the draft
 * fingerprint: every draft change advances the generation, and stale
 * responses never overwrite the current preview. Source pages execute
 * through the existing query paths; preview itself creates no state.
 */
export function createCompositionPreviewHandle(
  api: CompositionPreviewQueryApi = compositionApi,
) {
  const [state, setState] = createSignal<CompositionPreviewHandleState>({
    previewing: false,
    sources: {},
  });
  const [parameters, setParameters] = createSignal<Record<string, unknown>>({});

  let disposed = false;
  let generation = 0;
  let previewController: AbortController | undefined;
  const sourceControllers = new Map<string, AbortController>();
  const sourceGenerations = new Map<string, number>();
  const resolvedSources = new Map<string, CompositionResolvedSource>();
  let lastScope: CompositionPreviewScope | undefined;

  const isCurrent = (requestGeneration: number) =>
    !disposed && requestGeneration === generation;

  const abortSources = () => {
    for (const controller of sourceControllers.values()) controller.abort();
    sourceControllers.clear();
    sourceGenerations.clear();
    resolvedSources.clear();
  };

  const preview = async (
    spaceId: string,
    yaml: string,
    values: Record<string, unknown>,
    scope?: CompositionPreviewScope,
  ) => {
    const requestGeneration = ++generation;
    previewController?.abort();
    abortSources();
    lastScope = scope === undefined ? undefined : { ...scope };
    setParameters({ ...values });
    setState({
      yaml,
      spaceId,
      identity: undefined,
      previewing: true,
      preview: undefined,
      previewError: undefined,
      sources: {},
    });
    const controller = new AbortController();
    previewController = controller;
    try {
      const response = await api.preview(
        spaceId,
        yaml,
        values,
        controller.signal,
      );
      if (!isCurrent(requestGeneration) || controller.signal.aborted) return;
      // Every plan source stays registered for controller-owned paging, but
      // page execution covers the scoped fetch set only: visible-component
      // sources plus the Data-selected source. Unscoped previews keep the
      // previous fetch-all behavior.
      const wanted = scopedSourceIds(
        response.ok ? response.plan?.sources ?? [] : [],
        lastScope,
      );
      for (const source of response.ok ? response.plan?.sources ?? [] : []) {
        resolvedSources.set(source.source_id, source);
      }
      setState({
        yaml,
        spaceId,
        identity: response.draft_fingerprint
          ? { spaceId, fingerprint: response.draft_fingerprint }
          : undefined,
        previewing: false,
        preview: response,
        previewError: undefined,
        sources: Object.fromEntries(
          (response.ok ? response.plan?.sources ?? [] : [])
            .filter((source) => wanted.has(source.source_id))
            .map((source) => [
              source.source_id,
              { status: "loading", cursorStack: [undefined] },
            ]),
        ),
      });
      if (response.ok && response.plan) {
        await Promise.all(
          response.plan.sources
            .filter((source) => wanted.has(source.source_id))
            .map((source) =>
              requestSource(requestGeneration, spaceId, source, undefined, [
                undefined,
              ])
            ),
        );
      }
    } catch (error) {
      if (isCurrent(requestGeneration) && !controller.signal.aborted) {
        setState((current) => ({
          ...current,
          previewing: false,
          preview: undefined,
          previewError: error,
        }));
      }
    } finally {
      if (previewController === controller) previewController = undefined;
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
    const current = state();
    if (!current.yaml || !current.spaceId) return;
    const nextValues = { ...parameters() };
    if (value === undefined) delete nextValues[parameterId];
    else nextValues[parameterId] = value;
    void preview(current.spaceId, current.yaml, nextValues, lastScope);
  };

  /**
   * Fetch the page for a registered source on demand without re-previewing.
   * Navigator selection calls this for sources outside the scoped fetch set;
   * already-requested sources keep their state, and paging still flows
   * through the same ACL-checked request path.
   */
  const ensureSource = (sourceId: string) => {
    const current = state();
    if (!current.identity || current.sources[sourceId]) return;
    const source = resolvedSources.get(sourceId);
    if (!source) return;
    void requestSource(
      generation,
      current.identity.spaceId,
      source,
      undefined,
      [undefined],
    );
  };

  const next = (sourceId: string) => {
    const current = state();
    const sourceState = current.sources[sourceId];
    const source = resolvedSources.get(sourceId);
    const cursor = sourcePageNext(sourceState?.page);
    if (
      !sourceState || !source || !cursor || sourceState.status === "loading"
    ) return;
    if (!current.identity) return;
    void requestSource(
      generation,
      current.identity.spaceId,
      source,
      cursor,
      [...sourceState.cursorStack, cursor],
    );
  };

  const previous = (sourceId: string) => {
    const current = state();
    const sourceState = current.sources[sourceId];
    const source = resolvedSources.get(sourceId);
    if (!sourceState || !source || sourceState.cursorStack.length <= 1) return;
    if (!current.identity) return;
    const cursorStack = sourceState.cursorStack.slice(0, -1);
    void requestSource(
      generation,
      current.identity.spaceId,
      source,
      cursorStack.at(-1),
      cursorStack,
    );
  };

  const retry = (sourceId: string) => {
    const current = state();
    const sourceState = current.sources[sourceId];
    const source = resolvedSources.get(sourceId);
    if (!sourceState || !source || sourceState.status === "loading") return;
    if (!current.identity) return;
    void requestSource(
      generation,
      current.identity.spaceId,
      source,
      sourceState.cursor,
      sourceState.cursorStack,
    );
  };

  const dispose = () => {
    disposed = true;
    generation += 1;
    previewController?.abort();
    previewController = undefined;
    abortSources();
  };

  return {
    state,
    parameters,
    preview,
    setParameter,
    ensureSource,
    next,
    previous,
    retry,
    dispose,
  };
}
