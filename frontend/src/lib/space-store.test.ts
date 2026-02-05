// REQ-FE-001: Space selector
// REQ-FE-002: Automatic default space creation
// REQ-FE-003: Persist space selection
import { describe, it, expect, beforeEach, vi } from "vitest";
import { createRoot } from "solid-js";
import { createSpaceStore } from "./space-store";
import { resetMockData, seedSpace } from "~/test/mocks/handlers";
import type { Space } from "./types";

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

describe("createSpaceStore", () => {
	beforeEach(() => {
		resetMockData();
		localStorageMock.clear();
		vi.clearAllMocks();
	});

	it("should create default space when none exist", async () => {
		await createRoot(async (dispose) => {
			const store = createSpaceStore();

			expect(store.spaces()).toEqual([]);
			expect(store.initialized()).toBe(false);

			const selectedId = await store.loadSpaces();

			expect(selectedId).toBe("default");
			expect(store.spaces()).toHaveLength(1);
			expect(store.spaces()[0].id).toBe("default");
			expect(store.selectedSpaceId()).toBe("default");
			expect(store.initialized()).toBe(true);

			dispose();
		});
	});

	it("should select existing default space", async () => {
		const defaultWs: Space = {
			id: "default",
			name: "default",
			created_at: "2025-01-01T00:00:00Z",
		};
		seedSpace(defaultWs);

		await createRoot(async (dispose) => {
			const store = createSpaceStore();

			const selectedId = await store.loadSpaces();

			expect(selectedId).toBe("default");
			expect(store.spaces()).toHaveLength(1);
			expect(store.selectedSpaceId()).toBe("default");

			dispose();
		});
	});

	it("should restore persisted space selection", async () => {
		const ws1: Space = {
			id: "space-1",
			name: "Space One",
			created_at: "2025-01-01T00:00:00Z",
		};
		const ws2: Space = {
			id: "space-2",
			name: "Space Two",
			created_at: "2025-01-01T00:00:00Z",
		};
		seedSpace(ws1);
		seedSpace(ws2);

		// Simulate persisted selection
		localStorageMock.getItem.mockReturnValue("space-2");

		await createRoot(async (dispose) => {
			const store = createSpaceStore();

			const selectedId = await store.loadSpaces();

			expect(selectedId).toBe("space-2");
			expect(store.selectedSpaceId()).toBe("space-2");

			dispose();
		});
	});

	it("should create new space", async () => {
		const existingWs: Space = {
			id: "existing",
			name: "existing",
			created_at: "2025-01-01T00:00:00Z",
		};
		seedSpace(existingWs);

		await createRoot(async (dispose) => {
			const store = createSpaceStore();
			await store.loadSpaces();

			expect(store.spaces()).toHaveLength(1);

			const newWs = await store.createSpace("new-space");

			expect(newWs.id).toBe("new-space");
			expect(store.spaces()).toHaveLength(2);

			dispose();
		});
	});

	it("should select space and persist choice", async () => {
		const ws1: Space = {
			id: "space-1",
			name: "Space One",
			created_at: "2025-01-01T00:00:00Z",
		};
		const ws2: Space = {
			id: "space-2",
			name: "Space Two",
			created_at: "2025-01-01T00:00:00Z",
		};
		seedSpace(ws1);
		seedSpace(ws2);

		await createRoot(async (dispose) => {
			const store = createSpaceStore();
			await store.loadSpaces();

			store.selectSpace("space-2");

			expect(store.selectedSpaceId()).toBe("space-2");
			expect(localStorageMock.setItem).toHaveBeenCalledWith("ieapp-selected-space", "space-2");

			dispose();
		});
	});

	it("should not select non-existent space", async () => {
		const ws: Space = {
			id: "existing",
			name: "existing",
			created_at: "2025-01-01T00:00:00Z",
		};
		seedSpace(ws);

		await createRoot(async (dispose) => {
			const store = createSpaceStore();
			await store.loadSpaces();

			expect(store.selectedSpaceId()).toBe("existing");

			store.selectSpace("non-existent");

			// Should remain on existing space
			expect(store.selectedSpaceId()).toBe("existing");

			dispose();
		});
	});
});
