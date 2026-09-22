import { createSignal } from "solid-js";
import { createResource } from "./recoverable-resource";
import { formatUserFacingError } from "./user-facing-error";
import { type TranslationKey } from "./i18n";
import type { Entry, EntryRecord, EntryUpdatePayload } from "./types";
import { entryApi, RevisionConflictError } from "./ugoite-client";
import { pageFromArray } from "./pagination";

export const ENTRY_PAGE_SIZE = 100;

export interface EntryStoreState {
  entries: EntryRecord[];
  selectedEntryId: string | null;
  selectedEntry: Entry | null;
  loading: boolean;
  error: string | null;
  errorCause: unknown;
  // Optimistic state
  pendingUpdates: Map<string, EntryRecord>;
}

/**
 * Creates a reactive entry store for a space.
 * Implements optimistic updates with server reconciliation.
 */
export function createEntryStore(spaceId: () => string) {
  // Core state
  const [entries, setEntries] = createSignal<EntryRecord[]>([]);
  const [selectedEntryId, setSelectedEntryId] = createSignal<string | null>(
    null,
  );
  const [loading, setLoading] = createSignal(false);
  const [loadingMore, setLoadingMore] = createSignal(false);
  const [hasMore, setHasMore] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [errorCause, setErrorCause] = createSignal<unknown>(null);

  const clearError = () => {
    setError(null);
    setErrorCause(null);
  };

  const reportError = (
    cause: unknown,
    fallback: TranslationKey,
    operation: string,
  ) => {
    setErrorCause(cause);
    setError(formatUserFacingError(cause, fallback, operation));
  };

  // Track pending optimistic updates
  const pendingUpdates = new Map<
    string,
    { original: EntryRecord; optimistic: EntryRecord }
  >();

  // Fetch selected entry content
  const [selectedEntry, { refetch: refetchSelectedEntry }] = createResource(
    () => {
      const entryId = selectedEntryId();
      const wsId = spaceId();
      return entryId && wsId ? { wsId, entryId } : null;
    },
    async (params) => {
      /* v8 ignore start */
      if (!params) return null;
      /* v8 ignore stop */
      try {
        return await entryApi.get(params.wsId, params.entryId);
      } catch {
        return null;
      }
    },
  );

  /** Load the first page of entries from the server. */
  async function loadEntries() {
    setLoading(true);
    setLoadingMore(false);
    setHasMore(false);
    clearError();
    try {
      const fetchedEntries = await entryApi.list(
        spaceId(),
        ENTRY_PAGE_SIZE + 1,
      );
      const page = pageFromArray(fetchedEntries, ENTRY_PAGE_SIZE);
      setEntries(page.items);
      setHasMore(page.hasMore);
    } catch (e) {
      /* v8 ignore start */
      reportError(e, "entriesPage.failedLoad", "entry.list");
      /* v8 ignore stop */
    } finally {
      setLoading(false);
    }
  }

  /** Append the next server-ordered page without treating it as a total. */
  async function loadMoreEntries() {
    if (loading() || loadingMore() || !hasMore()) return;
    const offset = entries().length;
    setLoadingMore(true);
    clearError();
    try {
      const fetchedEntries = await entryApi.list(
        spaceId(),
        ENTRY_PAGE_SIZE + 1,
        offset,
      );
      const page = pageFromArray(fetchedEntries, ENTRY_PAGE_SIZE);
      setEntries((current) => [...current, ...page.items]);
      setHasMore(page.hasMore);
    } catch (e) {
      /* v8 ignore start */
      reportError(e, "entriesPage.failedLoad", "entry.list");
      /* v8 ignore stop */
    } finally {
      setLoadingMore(false);
    }
  }

  /** Update a entry with optimistic updates */
  async function updateEntry(entryId: string, payload: EntryUpdatePayload) {
    clearError();
    const currentEntries = entries();
    const entryIndex = currentEntries.findIndex((n) => n.id === entryId);

    if (entryIndex === -1) {
      const error = new Error("Entry not found in local state");
      reportError(error, "entryDetail.saveFailed", "entry.update");
      throw error;
    }

    const originalEntry = currentEntries[entryIndex];

    // Create optimistic record
    const optimisticEntry: EntryRecord = {
      ...originalEntry,
      updated_at: new Date().toISOString(),
      canvas_position: payload.canvas_position || originalEntry.canvas_position,
    };

    // Store for potential rollback
    pendingUpdates.set(entryId, {
      original: originalEntry,
      optimistic: optimisticEntry,
    });

    // Apply optimistic update
    setEntries((prev) =>
      prev.map((n) => (n.id === entryId ? optimisticEntry : n))
    );

    const wsId = spaceId();
    /* v8 ignore start */
    if (!wsId) {
      const error = new Error("Cannot update entry: space ID is missing");
      reportError(error, "entryDetail.savePrerequisite", "entry.update");
      throw error;
    }
    /* v8 ignore stop */

    try {
      const result = await entryApi.update(wsId, entryId, payload);

      // Clear pending update on success
      pendingUpdates.delete(entryId);

      // Entry: Do NOT refetch after save - this would cause the editor to lose
      // the user's current content and replace it with server content.
      // The caller (entries.tsx) maintains the editor state and should only
      // refetch when explicitly requested (e.g., on conflict resolution).

      return result;
    } catch (e) {
      // Rollback on failure
      /* v8 ignore start */
      const pending = pendingUpdates.get(entryId);
      if (pending) {
        setEntries((prev) =>
          prev.map((n) => (n.id === entryId ? pending.original : n))
        );
        pendingUpdates.delete(entryId);
      }
      /* v8 ignore stop */

      if (e instanceof RevisionConflictError) {
        // Reload to get server state
        await loadEntries();
        if (selectedEntryId() === entryId) {
          refetchSelectedEntry();
        }
      }

      /* v8 ignore start */
      reportError(e, "entriesPage.failedUpdate", "entry.update");
      /* v8 ignore stop */
      throw e;
    }
  }

  /** Delete a entry */
  async function deleteEntry(entryId: string) {
    clearError();

    // Optimistic removal
    const currentEntries = entries();
    const entryToDelete = currentEntries.find((n) => n.id === entryId);
    setEntries((prev) => prev.filter((n) => n.id !== entryId));

    // Clear selection if deleted
    if (selectedEntryId() === entryId) {
      setSelectedEntryId(null);
    }

    try {
      await entryApi.delete(spaceId(), entryId);
    } catch (e) {
      // Rollback on failure
      /* v8 ignore start */
      if (entryToDelete) {
        setEntries((prev) => [...prev, entryToDelete]);
      }
      /* v8 ignore stop */
      /* v8 ignore start */
      reportError(e, "entriesPage.failedDelete", "entry.delete");
      /* v8 ignore stop */
      throw e;
    }
  }

  /** Select a entry for editing */
  function selectEntry(entryId: string | null) {
    setSelectedEntryId(entryId);
  }

  return {
    // Reactive getters
    entries,
    selectedEntryId,
    selectedEntry,
    loading,
    loadingMore,
    hasMore,
    error,
    errorCause,

    // Actions
    loadEntries,
    loadMoreEntries,
    updateEntry,
    deleteEntry,
    selectEntry,
    refetchSelectedEntry,

    /**
     * Keyword search via the canonical EntryQuery text clause. Collection
     * reads never bypass entry.query; this helper performs a one-shot page
     * without mutating store state.
     */
    async searchEntries(query: string) {
      clearError();
      try {
        const page = await entryApi.query(spaceId(), {
          query: {
            scope: { kind: "all" },
            ...(query.trim() ? { text: query.trim() } : {}),
            filters: [],
            sort: [],
          },
          projection: { kind: "preview" },
          limit: ENTRY_PAGE_SIZE + 1,
        });
        return page.rows;
      } catch (e) {
        /* v8 ignore start */
        reportError(e, "entriesPage.failedSearch", "entry.query");
        /* v8 ignore stop */
        throw e;
      }
    },
  };
}

export type EntryStore = ReturnType<typeof createEntryStore>;
