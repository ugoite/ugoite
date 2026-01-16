import { createSignal } from "solid-js";
import type { Workspace } from "./types";
import { workspaceApi } from "./workspace-api";

const DEFAULT_WORKSPACE_ID = "default";
const STORAGE_KEY = "ieapp-selected-workspace";

/**
 * Creates a reactive workspace store.
 * Manages workspace listing, selection, and creation.
 */
export function createWorkspaceStore() {
	const [workspaces, setWorkspaces] = createSignal<Workspace[]>([]);
	const [selectedWorkspaceId, setSelectedWorkspaceIdInternal] = createSignal<string | null>(null);
	const [loading, setLoading] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);
	const [initialized, setInitialized] = createSignal(false);

	/** Get persisted workspace ID from localStorage */
	function getPersistedWorkspaceId(): string | null {
		if (typeof localStorage === "undefined") return null;
		return localStorage.getItem(STORAGE_KEY);
	}

	/** Persist workspace ID to localStorage */
	function persistWorkspaceId(id: string): void {
		if (typeof localStorage !== "undefined") {
			localStorage.setItem(STORAGE_KEY, id);
		}
	}

	/** Set selected workspace and persist */
	function setSelectedWorkspaceId(id: string | null): void {
		setSelectedWorkspaceIdInternal(id);
		if (id) {
			persistWorkspaceId(id);
		}
	}

	/** Load all workspaces and ensure a default exists */
	async function loadWorkspaces(): Promise<string> {
		setLoading(true);
		setError(null);
		try {
			const fetchedWorkspaces = await workspaceApi.list();
			setWorkspaces(fetchedWorkspaces);

			// Try to restore persisted workspace selection
			const persistedId = getPersistedWorkspaceId();
			if (persistedId && fetchedWorkspaces.some((w) => w.id === persistedId)) {
				setSelectedWorkspaceId(persistedId);
				setInitialized(true);
				return persistedId;
			}

			// If default workspace exists, select it
			const defaultWs = fetchedWorkspaces.find((w) => w.id === DEFAULT_WORKSPACE_ID);
			if (defaultWs) {
				setSelectedWorkspaceId(DEFAULT_WORKSPACE_ID);
				setInitialized(true);
				return DEFAULT_WORKSPACE_ID;
			}

			// If no workspaces exist, create the default one
			if (fetchedWorkspaces.length === 0) {
				const created = await createWorkspace(DEFAULT_WORKSPACE_ID);
				setSelectedWorkspaceId(created.id);
				setInitialized(true);
				return created.id;
			}

			// Otherwise, select the first available workspace
			const firstWs = fetchedWorkspaces[0];
			setSelectedWorkspaceId(firstWs.id);
			setInitialized(true);
			return firstWs.id;
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to load workspaces");
			throw e;
		} finally {
			setLoading(false);
		}
	}

	/** Create a new workspace */
	async function createWorkspace(name: string): Promise<Workspace> {
		setError(null);
		try {
			const result = await workspaceApi.create(name);
			// Reload workspaces to get full workspace object
			await loadWorkspacesOnly();
			const newWs = workspaces().find((w) => w.id === result.id);
			return (
				newWs || {
					id: result.id,
					name: result.name,
					created_at: new Date().toISOString(),
				}
			);
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to create workspace");
			throw e;
		}
	}

	/** Load workspaces without changing selection */
	async function loadWorkspacesOnly(): Promise<void> {
		try {
			const fetchedWorkspaces = await workspaceApi.list();
			setWorkspaces(fetchedWorkspaces);
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to load workspaces");
		}
	}

	/** Select a workspace */
	function selectWorkspace(workspaceId: string): void {
		if (workspaces().some((w) => w.id === workspaceId)) {
			setSelectedWorkspaceId(workspaceId);
		}
	}

	return {
		// Reactive getters
		workspaces,
		selectedWorkspaceId,
		loading,
		error,
		initialized,

		// Actions
		loadWorkspaces,
		createWorkspace,
		selectWorkspace,
	};
}

export type WorkspaceStore = ReturnType<typeof createWorkspaceStore>;
