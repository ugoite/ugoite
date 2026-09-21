import { type Accessor, createSignal, untrack } from "solid-js";
import { entryApi } from "./entry-api";

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

type QueryPage = (
  spaceId: string,
  request: EntryPageRequest,
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

  const loadAt = async (
    after: string | undefined,
    nextStack: (string | undefined)[],
  ) => {
    const generation = ++requestGeneration;
    setLoading(true);
    setError(null);
    try {
      const page = await queryPage(untrack(spaceId), {
        query: untrack(query),
        projection: untrack(projection),
        limit: pageSize,
        ...(after ? { after } : {}),
      });
      if (generation !== requestGeneration) return;
      setRows(page.rows);
      setHasMore(page.has_more);
      setNextCursor(page.next);
      setCurrentStart(after);
      setCursorStack(nextStack);
    } catch (cause) {
      if (generation === requestGeneration) setError(cause);
    } finally {
      if (generation === requestGeneration) setLoading(false);
    }
  };

  const load = async () => await loadAt(undefined, [undefined]);

  const updateQuery = (updater: (current: EntryQuery) => EntryQuery) => {
    const current = untrack(query);
    const next = updater(current);
    if (JSON.stringify(current) === JSON.stringify(next)) return;
    setQuery(next);
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
    await loadAt(untrack(currentStart), untrack(cursorStack));

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
    invalidate,
    next,
    previous,
    setText,
    setFilters,
    setSort,
    setScope,
    setProjection: changeProjection,
    configure,
  };
}
