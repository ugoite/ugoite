// REQ-FE-001: Workspace selector
// REQ-FE-002: Automatic default workspace creation
// REQ-FE-003: Persist workspace selection
import { describe, it, expect, beforeEach, vi } from "vitest";
import { createRoot } from "solid-js";
import { createWorkspaceStore } from "./workspace-store";
import { resetMockData, seedWorkspace } from "~/test/mocks/handlers";
import type { Workspace } from "./types";

// Mock localStorage
const localStorageMock = (() => {
	let store: Record<string, string> = {};
	return {
		getItem: vi.fn((key: string) => store[key] || null),
		setItem: vi.fn((key: string, value: string) => {
			store[key] = value;
		}),
		clear: () => {
			store = {};
		},
	};
})();

Object.defineProperty(globalThis, "localStorage", {
	value: localStorageMock,
});

describe("createWorkspaceStore", () => {
	beforeEach(() => {
		resetMockData();
		localStorageMock.clear();
		vi.clearAllMocks();
	});

	it("should create default workspace when none exist", async () => {
		await createRoot(async (dispose) => {
			const store = createWorkspaceStore();

			expect(store.workspaces()).toEqual([]);
			expect(store.initialized()).toBe(false);

			const selectedId = await store.loadWorkspaces();

			expect(selectedId).toBe("default");
			expect(store.workspaces()).toHaveLength(1);
			expect(store.workspaces()[0].id).toBe("default");
			expect(store.selectedWorkspaceId()).toBe("default");
			expect(store.initialized()).toBe(true);

			dispose();
		});
	});

	it("should select existing default workspace", async () => {
		const defaultWs: Workspace = {
			id: "default",
			name: "default",
			created_at: "2025-01-01T00:00:00Z",
		};
		seedWorkspace(defaultWs);

		await createRoot(async (dispose) => {
			const store = createWorkspaceStore();

			const selectedId = await store.loadWorkspaces();

			expect(selectedId).toBe("default");
			expect(store.workspaces()).toHaveLength(1);
			expect(store.selectedWorkspaceId()).toBe("default");

			dispose();
		});
	});

	it("should restore persisted workspace selection", async () => {
		const ws1: Workspace = {
			id: "workspace-1",
			name: "Workspace One",
			created_at: "2025-01-01T00:00:00Z",
		};
		const ws2: Workspace = {
			id: "workspace-2",
			name: "Workspace Two",
			created_at: "2025-01-01T00:00:00Z",
		};
		seedWorkspace(ws1);
		seedWorkspace(ws2);

		// Simulate persisted selection
		localStorageMock.getItem.mockReturnValue("workspace-2");

		await createRoot(async (dispose) => {
			const store = createWorkspaceStore();

			const selectedId = await store.loadWorkspaces();

			expect(selectedId).toBe("workspace-2");
			expect(store.selectedWorkspaceId()).toBe("workspace-2");

			dispose();
		});
	});

	it("should create new workspace", async () => {
		const existingWs: Workspace = {
			id: "existing",
			name: "existing",
			created_at: "2025-01-01T00:00:00Z",
		};
		seedWorkspace(existingWs);

		await createRoot(async (dispose) => {
			const store = createWorkspaceStore();
			await store.loadWorkspaces();

			expect(store.workspaces()).toHaveLength(1);

			const newWs = await store.createWorkspace("new-workspace");

			expect(newWs.id).toBe("new-workspace");
			expect(store.workspaces()).toHaveLength(2);

			dispose();
		});
	});

	it("should select workspace and persist choice", async () => {
		const ws1: Workspace = {
			id: "workspace-1",
			name: "Workspace One",
			created_at: "2025-01-01T00:00:00Z",
		};
		const ws2: Workspace = {
			id: "workspace-2",
			name: "Workspace Two",
			created_at: "2025-01-01T00:00:00Z",
		};
		seedWorkspace(ws1);
		seedWorkspace(ws2);

		await createRoot(async (dispose) => {
			const store = createWorkspaceStore();
			await store.loadWorkspaces();

			store.selectWorkspace("workspace-2");

			expect(store.selectedWorkspaceId()).toBe("workspace-2");
			expect(localStorageMock.setItem).toHaveBeenCalledWith(
				"ieapp-selected-workspace",
				"workspace-2",
			);

			dispose();
		});
	});

	it("should not select non-existent workspace", async () => {
		const ws: Workspace = {
			id: "existing",
			name: "existing",
			created_at: "2025-01-01T00:00:00Z",
		};
		seedWorkspace(ws);

		await createRoot(async (dispose) => {
			const store = createWorkspaceStore();
			await store.loadWorkspaces();

			expect(store.selectedWorkspaceId()).toBe("existing");

			store.selectWorkspace("non-existent");

			// Should remain on existing workspace
			expect(store.selectedWorkspaceId()).toBe("existing");

			dispose();
		});
	});
});
