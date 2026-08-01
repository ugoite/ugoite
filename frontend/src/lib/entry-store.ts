import { createSignal } from "solid-js";
import { createResource } from "./recoverable-resource";
import { formatUserFacingError } from "./user-facing-error";
import { type TranslationKey } from "./i18n";
import type { Entry, EntryRecord, EntryUpdatePayload } from "./types";
import { entryApi, RevisionConflictError } from "./ugoite-client";
import { searchApi } from "./ugoite-client";

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

  /** Load all entries from server */
  async function loadEntries() {
    setLoading(true);
    clearError();
    try {
      const fetchedEntries = await entryApi.list(spaceId());
      setEntries(fetchedEntries);
    } catch (e) {
      /* v8 ignore start */
      reportError(e, "entriesPage.failedLoad", "entry.list");
      /* v8 ignore stop */
    } finally {
      setLoading(false);
    }
  }

  /** Create a new entry */
  async function createEntry(content: string, id?: string) {
    clearError();
    try {
      const result = await entryApi.create(spaceId(), {
        markdown: content,
        id,
      });
      // Reload to get the indexed version
      await loadEntries();
      return result;
    } catch (e) {
      /* v8 ignore start */
      reportError(e, "entriesPage.failedCreate", "entry.create");
      /* v8 ignore stop */
      throw e;
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

    // Extract title from markdown for optimistic update
    // Use indexOf-based extraction to prevent ReDoS vulnerability
    let title = originalEntry.title;
    const lines = payload.markdown.split(/\r?\n/);
    for (const line of lines) {
      if (line.startsWith("# ") || line.startsWith("#\t")) {
        const spaceIdx = line.indexOf(" ");
        const tabIdx = line.indexOf("\t");
        const idx = spaceIdx !== -1 ? spaceIdx : tabIdx;
        title = line.slice(idx + 1).trim();
        break;
      }
    }

    // Create optimistic record
    const optimisticEntry: EntryRecord = {
      ...originalEntry,
      title,
      updated_at: new Date().toISOString(),
      canvas_position: payload.canvas_position || originalEntry.canvas_position,
      assets: payload.assets ?? originalEntry.assets,
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
    error,
    errorCause,

    // Actions
    loadEntries,
    createEntry,
    updateEntry,
    deleteEntry,
    selectEntry,
    refetchSelectedEntry,

    /** Perform a keyword search without mutating store state */
    async searchEntries(query: string) {
      clearError();
      try {
        return await searchApi.keyword(spaceId(), query);
      } catch (e) {
        /* v8 ignore start */
        reportError(e, "entriesPage.failedSearch", "search.keyword");
        /* v8 ignore stop */
        throw e;
      }
    },
  };
}

export type EntryStore = ReturnType<typeof createEntryStore>;
