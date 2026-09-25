import {
  type Accessor,
  createEffect,
  createSignal,
  onCleanup,
  untrack,
} from "solid-js";
import { entryApi } from "~/lib/ugoite-client";

/** Logical EntryQuery types mirrored from ugoite-core's serde contract. */
export type EntryQueryScope =
  | { kind: "all" }
  | { kind: "form"; form_id: string };

export type EntryFieldRef =
  | { kind: "property"; field_id: number }
  | { kind: "form" }
  | { kind: "created_at" }
  | { kind: "updated_at" };

export type EntrySortDirection = "asc" | "desc";
export type EntryFilterOperator =
  | "equals"
  | "contains"
  | "lt"
  | "lte"
  | "gt"
  | "gte";

export interface EntryFilter {
  field: EntryFieldRef;
  operator: EntryFilterOperator;
  value: unknown;
}

export interface EntrySort {
  field: EntryFieldRef;
  direction: EntrySortDirection;
}

export interface EntryQuery {
  scope: EntryQueryScope;
  text?: string;
  filters: EntryFilter[];
  sort: EntrySort[];
}

export type EntryProjection =
  | { kind: "fields"; fields: EntryFieldRef[] }
  | { kind: "preview" };

export interface EntryPageRequest {
  query: EntryQuery;
  projection: EntryProjection;
  limit: number;
  after?: string;
}

export interface EntryCountRequest {
  query: EntryQuery;
}

export interface EntryQueryResult {
  id: string;
  form_id: string;
  revision_id: string;
  created_at_micros: number;
  updated_at_micros: number;
  properties?: Record<string, unknown>;
  preview?: string;
}

export interface EntryPage {
  rows: EntryQueryResult[];
  has_more: boolean;
  next?: string;
}

export interface EntryCount {
  count: number;
}

export interface EntryFieldCapability {
  field: EntryFieldRef;
  name: string;
  field_type: string;
  filterable: boolean;
  sortable: boolean;
  projectable: boolean;
  supported_operators: EntryFilterOperator[];
}

export interface EntryQueryCapabilities {
  scope: EntryQueryScope;
  fields: EntryFieldCapability[];
}

export const allEntryScope = (): EntryQueryScope => ({ kind: "all" });

export const previewProjection = (): EntryProjection => ({ kind: "preview" });

export const systemEntryCapabilities = (
  scope: EntryQueryScope = allEntryScope(),
): EntryQueryCapabilities => ({
  scope,
  fields: [
    ...(scope.kind === "all"
      ? [{
        field: { kind: "form" } as const,
        name: "Form",
        field_type: "form",
        filterable: false,
        sortable: true,
        projectable: true,
        supported_operators: [],
      }]
      : []),
    {
      field: { kind: "created_at" },
      name: "Created",
      field_type: "timestamp",
      filterable: true,
      sortable: true,
      projectable: true,
      supported_operators: ["lt", "lte", "gt", "gte", "equals"],
    },
    {
      field: { kind: "updated_at" },
      name: "Updated",
      field_type: "timestamp",
      filterable: true,
      sortable: true,
      projectable: true,
      supported_operators: ["lt", "lte", "gt", "gte", "equals"],
    },
  ],
});

export type EntryQueryController = ReturnType<
  typeof createEntryQueryController
>;

const isAbortError = (error: unknown): boolean =>
  !!error && typeof error === "object" &&
  (error as { name?: unknown }).name === "AbortError";

type QueryPage = (
  spaceId: string,
  request: EntryPageRequest,
  signal: AbortSignal,
) => Promise<EntryPage>;

/**
 * Client-held EntryQuery state. The controller owns no query result on the
 * server: page starts and continuation tokens are disposable browser state.
 */
export function createEntryQueryController(
  spaceId: Accessor<string>,
  initialQuery: EntryQuery = {
    scope: allEntryScope(),
    filters: [],
    sort: [],
  },
  initialProjection: EntryProjection = previewProjection(),
  pageSize = 50,
  queryPage: QueryPage = entryApi.query,
) {
  const [query, setQuery] = createSignal<EntryQuery>(initialQuery);
  const [projection, setProjection] = createSignal<EntryProjection>(
    initialProjection,
  );
  const [rows, setRows] = createSignal<EntryQueryResult[]>([]);
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<unknown>(null);
  const [hasMore, setHasMore] = createSignal(false);
  const [nextCursor, setNextCursor] = createSignal<string | undefined>();
  const [currentStart, setCurrentStart] = createSignal<string | undefined>();
  const [cursorStack, setCursorStack] = createSignal<(string | undefined)[]>([
    undefined,
  ]);
  let requestGeneration = 0;
  let activeController: AbortController | undefined;
  let activeRequestKey = "";
  let hasRequested = false;
  let failedRead: {
    after: string | undefined;
    cursorStack: (string | undefined)[];
  } | undefined;

  const cancel = () => {
    requestGeneration += 1;
    activeController?.abort();
    activeController = undefined;
    activeRequestKey = "";
    hasRequested = false;
    failedRead = undefined;
    setLoading(false);
  };

  let previousSpaceId = untrack(spaceId);
  createEffect(() => {
    const currentSpaceId = spaceId();
    if (currentSpaceId === previousSpaceId) return;
    previousSpaceId = currentSpaceId;
    failedRead = undefined;
    setRows([]);
    setError(null);
    setHasMore(false);
    setNextCursor(undefined);
    setCurrentStart(undefined);
    setCursorStack([undefined]);
    if (hasRequested) void loadAt(undefined, [undefined]);
  });

  onCleanup(cancel);

  const loadAt = async (
    after: string | undefined,
    nextStack: (string | undefined)[],
    clearRows = after === undefined && nextStack.length === 1,
  ) => {
    hasRequested = true;
    const requestSpaceId = untrack(spaceId);
    const requestKey = JSON.stringify({
      spaceId: requestSpaceId,
      query: untrack(query),
      projection: untrack(projection),
      after,
    });
    if (activeController && activeRequestKey === requestKey) return;
    const generation = ++requestGeneration;
    activeController?.abort();
    const controller = new AbortController();
    activeController = controller;
    activeRequestKey = requestKey;
    failedRead = undefined;
    setLoading(true);
    setError(null);
    if (clearRows) {
      setRows([]);
      setHasMore(false);
      setNextCursor(undefined);
    }
    try {
      const page = await queryPage(requestSpaceId, {
        query: untrack(query),
        projection: untrack(projection),
        limit: pageSize,
        ...(after ? { after } : {}),
      }, controller.signal);
      if (generation !== requestGeneration) return;
      setRows(page.rows);
      setHasMore(page.has_more);
      setNextCursor(page.next);
      setCurrentStart(after);
      setCursorStack(nextStack);
    } catch (cause) {
      if (
        generation === requestGeneration && !controller.signal.aborted &&
        !isAbortError(cause)
      ) {
        failedRead = { after, cursorStack: nextStack };
        setError(cause);
      }
    } finally {
      if (generation === requestGeneration) {
        if (activeController === controller) activeController = undefined;
        if (activeRequestKey === requestKey) activeRequestKey = "";
        setLoading(false);
      }
    }
  };

  const load = async () => await loadAt(undefined, [undefined]);

  const updateQuery = (updater: (current: EntryQuery) => EntryQuery) => {
    const current = untrack(query);
    const next = updater(current);
    if (JSON.stringify(current) === JSON.stringify(next)) return;
    setQuery(next);
    setRows([]);
    setHasMore(false);
    setNextCursor(undefined);
    setCurrentStart(undefined);
    setCursorStack([undefined]);
    void loadAt(undefined, [undefined]);
  };

  const setText = (text: string) => {
    const normalized = text.trim();
    updateQuery((current) => ({
      ...current,
      ...(normalized ? { text: normalized } : { text: undefined }),
    }));
  };

  const setFilters = (filters: EntryFilter[]) => {
    updateQuery((current) => ({ ...current, filters }));
  };

  const setSort = (sort: EntrySort[]) => {
    updateQuery((current) => ({ ...current, sort }));
  };

  const setScope = (scope: EntryQueryScope) => {
    updateQuery((current) => ({ ...current, scope }));
  };

  const changeProjection = (next: EntryProjection) => {
    if (JSON.stringify(untrack(projection)) === JSON.stringify(next)) return;
    setProjection(next);
    // Projection is not query identity. Re-read the same page coordinate.
    void loadAt(untrack(currentStart), untrack(cursorStack));
  };

  const next = async () => {
    const continuation = untrack(nextCursor);
    if (!continuation || untrack(loading)) return;
    await loadAt(continuation, [...untrack(cursorStack), continuation]);
  };

  const previous = async () => {
    const stack = untrack(cursorStack);
    if (stack.length <= 1 || untrack(loading)) return;
    const nextStack = stack.slice(0, -1);
    await loadAt(nextStack[nextStack.length - 1], nextStack);
  };

  const refresh = async () =>
    await loadAt(untrack(currentStart), untrack(cursorStack), false);

  const retry = async () => {
    const failed = failedRead;
    if (!failed) return await refresh();
    await loadAt(failed.after, failed.cursorStack, false);
  };

  const invalidate = async () => await load();

  const configure = async (
    nextQuery: EntryQuery,
    nextProjection: EntryProjection,
  ) => {
    const queryChanged =
      JSON.stringify(untrack(query)) !== JSON.stringify(nextQuery);
    const projectionChanged =
      JSON.stringify(untrack(projection)) !== JSON.stringify(nextProjection);
    if (queryChanged) setQuery(nextQuery);
    if (projectionChanged) setProjection(nextProjection);
    if (queryChanged) {
      setRows([]);
      setHasMore(false);
      setNextCursor(undefined);
      setCurrentStart(undefined);
      setCursorStack([undefined]);
    }
    await loadAt(undefined, [undefined]);
  };

  return {
    query,
    projection,
    rows,
    loading,
    error,
    hasMore,
    nextCursor,
    currentStart,
    cursorStack,
    canGoPrevious: () => cursorStack().length > 1,
    load,
    refresh,
    retry,
    invalidate,
    next,
    previous,
    setText,
    setFilters,
    setSort,
    setScope,
    setProjection: changeProjection,
    configure,
    cancel,
  };
}
