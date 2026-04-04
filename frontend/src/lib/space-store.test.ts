// REQ-FE-001: Space selector
// REQ-FE-002: No client-side default space creation
// REQ-FE-003: Persist space selection
import { describe, it, expect, beforeEach, vi } from "vitest";
import { createRoot } from "solid-js";
import { waitFor } from "@solidjs/testing-library";
import { http, HttpResponse } from "msw";
import { createSpaceStore } from "./space-store";
import {
	getPreferencePatches,
	resetMockData,
	seedPreferences,
	seedSpace,
} from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";
import type { Space } from "./types";
import { testApiUrl } from "~/test/http-origin";

// Mock localStorage
const localStorageMock = (() => {
	let store: Record<string, string> = {};
	return {
		getItem: vi.fn((key: string) => store[key] || null),
		setItem: vi.fn((key: string, value: string) => {
			store[key] = value;
		}),
		removeItem: vi.fn((key: string) => {
			delete store[key];
		}),
		clear: () => {
			store = {};
		},
	};
})();

Object.defineProperty(globalThis, "localStorage", {
	value: localStorageMock,
	configurable: true,
});

Object.defineProperty(window, "localStorage", {
	value: localStorageMock,
	configurable: true,
});

describe("createSpaceStore", () => {
	beforeEach(async () => {
		resetMockData();
		localStorageMock.clear();
		vi.clearAllMocks();
		const { resetPortablePreferencesState } = await import("./preferences-store");
		resetPortablePreferencesState();
	});

	it("should keep selection empty when no spaces exist", async () => {
		await createRoot(async (dispose) => {
			const store = createSpaceStore();

			expect(store.spaces()).toEqual([]);
			expect(store.initialized()).toBe(false);

			const selectedId = await store.loadSpaces();

			expect(selectedId).toBe("");
			expect(store.spaces()).toHaveLength(0);
			expect(store.selectedSpaceId()).toBeNull();
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

		localStorageMock.setItem("ugoite-selected-space", "space-2");

		await createRoot(async (dispose) => {
			const store = createSpaceStore();

			const selectedId = await store.loadSpaces();

			expect(selectedId).toBe("space-2");
			expect(store.selectedSpaceId()).toBe("space-2");

			dispose();
		});
	});

	it("REQ-FE-003: should prefer portable selected space preference", async () => {
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
		const portableSelection = {} as import("./types").UserPreferencesPatchPayload;
		portableSelection.selected_space_id = "space-2";
		seedPreferences(portableSelection);
		localStorageMock.setItem("ugoite-selected-space", "space-1");

		const { initializePortablePreferences } = await import("./preferences-store");
		await initializePortablePreferences();

		await createRoot(async (dispose) => {
			const store = createSpaceStore();

			const selectedId = await store.loadSpaces();

			expect(selectedId).toBe("space-2");
			expect(store.selectedSpaceId()).toBe("space-2");

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
			expect(localStorageMock.setItem).toHaveBeenCalledWith("ugoite-selected-space", "space-2");
			await waitFor(() => {
				const expectedPatch = {} as import("./types").UserPreferencesPatchPayload;
				expectedPatch.selected_space_id = "space-2";
				expect(getPreferencePatches()).toContainEqual(expectedPatch);
			});

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

	it("should throw and set error on loadSpaces failure", async () => {
		server.use(
			http.get(testApiUrl("/spaces"), () =>
				HttpResponse.json({ detail: "Server error" }, { status: 500 }),
			),
		);
		await createRoot(async (dispose) => {
			const store = createSpaceStore();
			await expect(store.loadSpaces()).rejects.toThrow("Failed to list spaces");
			expect(store.error()).toContain("Failed to list spaces");
			dispose();
		});
	});
});
