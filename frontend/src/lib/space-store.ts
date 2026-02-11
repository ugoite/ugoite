import { createSignal } from "solid-js";
import type { Space } from "./types";
import { spaceApi } from "./space-api";

const DEFAULT_SPACE_ID = "default";
const STORAGE_KEY = "ugoite-selected-space";

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

	/** Get persisted space ID from localStorage */
	function getPersistedSpaceId(): string | null {
		if (typeof localStorage === "undefined") return null;
		return localStorage.getItem(STORAGE_KEY);
	}

	/** Persist space ID to localStorage */
	function persistSpaceId(id: string): void {
		if (typeof localStorage !== "undefined") {
			localStorage.setItem(STORAGE_KEY, id);
		}
	}

	/** Set selected space and persist */
	function setSelectedSpaceId(id: string | null): void {
		setSelectedSpaceIdInternal(id);
		if (id) {
			persistSpaceId(id);
		}
	}

	/** Load all spaces and ensure a default exists */
	async function loadSpaces(): Promise<string> {
		setLoading(true);
		setError(null);
		try {
			const fetchedSpaces = await spaceApi.list();
			setSpaces(fetchedSpaces);

			// Try to restore persisted space selection
			const persistedId = getPersistedSpaceId();
			if (persistedId && fetchedSpaces.some((space) => space.id === persistedId)) {
				setSelectedSpaceId(persistedId);
				setInitialized(true);
				return persistedId;
			}

			// If default space exists, select it
			const defaultSpace = fetchedSpaces.find((space) => space.id === DEFAULT_SPACE_ID);
			if (defaultSpace) {
				setSelectedSpaceId(DEFAULT_SPACE_ID);
				setInitialized(true);
				return DEFAULT_SPACE_ID;
			}

			// If no spaces exist, create the default one
			if (fetchedSpaces.length === 0) {
				const created = await createSpace(DEFAULT_SPACE_ID);
				setSelectedSpaceId(created.id);
				setInitialized(true);
				return created.id;
			}

			// Otherwise, select the first available space
			const firstSpace = fetchedSpaces[0];
			setSelectedSpaceId(firstSpace.id);
			setInitialized(true);
			return firstSpace.id;
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to load spaces");
			throw e;
		} finally {
			setLoading(false);
		}
	}

	/** Create a new space */
	async function createSpace(name: string): Promise<Space> {
		setError(null);
		try {
			const result = await spaceApi.create(name);
			// Reload spaces to get full space object
			await loadSpacesOnly();
			const newSpace = spaces().find((space) => space.id === result.id);
			return (
				newSpace || {
					id: result.id,
					name: result.name,
					created_at: new Date().toISOString(),
				}
			);
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to create space");
			throw e;
		}
	}

	/** Load spaces without changing selection */
	async function loadSpacesOnly(): Promise<void> {
		try {
			const fetchedSpaces = await spaceApi.list();
			setSpaces(fetchedSpaces);
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to load spaces");
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
		createSpace,
		selectSpace,
	};
}

export type SpaceStore = ReturnType<typeof createSpaceStore>;
