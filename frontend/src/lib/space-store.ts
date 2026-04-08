import { createSignal } from "solid-js";
import { readLocalPreferences, writeLocalPreferences } from "./preferences-local";
import { portablePreferences, setSelectedSpacePreference } from "~/lib/preferences-store";
import { DEFAULT_SPACE_ID, isReservedAdminSpace, sortSpaces } from "./space-list";
import type { Space } from "./types";
import { spaceApi } from "./space-api";

/**
 * Creates a reactive space store.
 * Manages space listing, selection, and creation.
 */
export function createSpaceStore() {
	const [spaces, setSpaces] = createSignal<Space[]>([]);
	const [selectedSpaceId, setSelectedSpaceIdInternal] = createSignal<string | null>(null);
	const [loading, setLoading] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);
	const [initialized, setInitialized] = createSignal(false);

	/** Get preferred space ID from portable preferences with local fallback */
	function getPersistedSpaceId(): string | null {
		return portablePreferences().selected_space_id ?? readLocalPreferences().selected_space_id;
	}

	/** Persist space ID to portable preferences and local fallback storage */
	function persistSpaceId(id: string | null): void {
		const patch = {} as import("./types").UserPreferencesPatchPayload;
		patch.selected_space_id = id;
		writeLocalPreferences(patch);
		void setSelectedSpacePreference(id);
	}

	/** Set selected space and persist */
	function setSelectedSpaceId(id: string | null, persist = true): void {
		setSelectedSpaceIdInternal(id);
		if (persist) {
			persistSpaceId(id);
		}
	}

	/** Load all spaces and select an available space */
	async function loadSpaces(): Promise<string> {
		setLoading(true);
		setError(null);
		try {
			const fetchedSpaces = sortSpaces(await spaceApi.list());
			setSpaces(fetchedSpaces);
			const selectableSpaces = fetchedSpaces.filter((space) => !isReservedAdminSpace(space));

			// Try to restore persisted space selection
			const persistedId = getPersistedSpaceId();
			const repairingPersistedSelection =
				persistedId !== null && !selectableSpaces.some((space) => space.id === persistedId);
			if (persistedId && selectableSpaces.some((space) => space.id === persistedId)) {
				setSelectedSpaceId(persistedId, false);
				setInitialized(true);
				return persistedId;
			}

			// If default space exists, select it
			const defaultSpace = selectableSpaces.find((space) => space.id === DEFAULT_SPACE_ID);
			if (defaultSpace) {
				setSelectedSpaceId(DEFAULT_SPACE_ID, repairingPersistedSelection);
				setInitialized(true);
				return DEFAULT_SPACE_ID;
			}

			// No client-side space creation; remain unselected when no user-facing spaces exist
			if (selectableSpaces.length === 0) {
				setSelectedSpaceId(null, repairingPersistedSelection);
				setInitialized(true);
				return "";
			}

			// Otherwise, select the first available user-facing space
			const firstSpace = selectableSpaces[0];
			setSelectedSpaceId(firstSpace.id, repairingPersistedSelection);
			setInitialized(true);
			return firstSpace.id;
		} catch (e) {
			/* v8 ignore start */
			setError(e instanceof Error ? e.message : "Failed to load spaces");
			/* v8 ignore stop */
			throw e;
		} finally {
			setLoading(false);
		}
	}

	/** Select a space */
	function selectSpace(spaceId: string): void {
		if (spaces().some((space) => space.id === spaceId)) {
			setSelectedSpaceId(spaceId);
		}
	}

	return {
		// Reactive getters
		spaces,
		selectedSpaceId,
		loading,
		error,
		initialized,

		// Actions
		loadSpaces,
		selectSpace,
	};
}

export type SpaceStore = ReturnType<typeof createSpaceStore>;
