import { createSignal, createResource } from "solid-js";
import type { Entry, EntryRecord, EntryUpdatePayload } from "./types";
import { entryApi, RevisionConflictError } from "./entry-api";
import { searchApi } from "./search-api";

export interface EntryStoreState {
	entries: EntryRecord[];
	selectedEntryId: string | null;
	selectedEntry: Entry | null;
	loading: boolean;
	error: string | null;
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
	const [selectedEntryId, setSelectedEntryId] = createSignal<string | null>(null);
	const [loading, setLoading] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);

	// Track pending optimistic updates
	const pendingUpdates = new Map<string, { original: EntryRecord; optimistic: EntryRecord }>();

	// Fetch selected entry content
	const [selectedEntry, { refetch: refetchSelectedEntry }] = createResource(
		() => {
			const entryId = selectedEntryId();
			const wsId = spaceId();
			return entryId && wsId ? { wsId, entryId } : null;
		},
		async (params) => {
			if (!params) return null;
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
		setError(null);
		try {
			const fetchedEntries = await entryApi.list(spaceId());
			setEntries(fetchedEntries);
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to load entries");
		} finally {
			setLoading(false);
		}
	}

	/** Create a new entry */
	async function createEntry(content: string, id?: string) {
		setError(null);
		try {
			const result = await entryApi.create(spaceId(), { content, id });
			// Reload to get the indexed version
			await loadEntries();
			return result;
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to create entry");
			throw e;
		}
	}

	/** Update a entry with optimistic updates */
	async function updateEntry(entryId: string, payload: EntryUpdatePayload) {
		setError(null);
		const currentEntries = entries();
		const entryIndex = currentEntries.findIndex((n) => n.id === entryId);

		if (entryIndex === -1) {
			throw new Error("Entry not found in local state");
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
		pendingUpdates.set(entryId, { original: originalEntry, optimistic: optimisticEntry });

		// Apply optimistic update
		setEntries((prev) => prev.map((n) => (n.id === entryId ? optimisticEntry : n)));

		const wsId = spaceId();
		if (!wsId) {
			const error = new Error("Cannot update entry: space ID is missing");
			setError(error.message);
			throw error;
		}

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
			const pending = pendingUpdates.get(entryId);
			if (pending) {
				setEntries((prev) => prev.map((n) => (n.id === entryId ? pending.original : n)));
				pendingUpdates.delete(entryId);
			}

			if (e instanceof RevisionConflictError) {
				// Reload to get server state
				await loadEntries();
				if (selectedEntryId() === entryId) {
					refetchSelectedEntry();
				}
			}

			setError(e instanceof Error ? e.message : "Failed to update entry");
			throw e;
		}
	}

	/** Delete a entry */
	async function deleteEntry(entryId: string) {
		setError(null);

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
			if (entryToDelete) {
				setEntries((prev) => [...prev, entryToDelete]);
			}
			setError(e instanceof Error ? e.message : "Failed to delete entry");
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

		// Actions
		loadEntries,
		createEntry,
		updateEntry,
		deleteEntry,
		selectEntry,
		refetchSelectedEntry,

		/** Perform a keyword search without mutating store state */
		async searchEntries(query: string) {
			try {
				return await searchApi.keyword(spaceId(), query);
			} catch (e) {
				setError(e instanceof Error ? e.message : "Failed to search entries");
				throw e;
			}
		},
	};
}

export type EntryStore = ReturnType<typeof createEntryStore>;
