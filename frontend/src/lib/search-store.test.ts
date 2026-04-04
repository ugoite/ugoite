// REQ-FE-030: Search store
import { describe, it, expect, beforeEach } from "vitest";
import { createRoot } from "solid-js";
import { http, HttpResponse } from "msw";
import { createSearchStore } from "./search-store";
import { resetMockData, seedEntry, seedSpace } from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";
import type { Entry, EntryRecord, Space } from "./types";
import { testApiUrl } from "~/test/http-origin";

const testSpace: Space = {
	id: "search-store-ws",
	name: "Search Store Space",
	created_at: "2025-01-01T00:00:00Z",
};

describe("createSearchStore", () => {
	beforeEach(() => {
		resetMockData();
		seedSpace(testSpace);
		const entry: Entry = {
			id: "entry-1",
			content: "# Test Entry\nContent about rockets",
			revision_id: "rev-1",
			created_at: "2025-01-01T00:00:00Z",
			updated_at: "2025-01-01T00:00:00Z",
		};
		const record: EntryRecord = {
			id: "entry-1",
			title: "Test Entry",
			updated_at: "2025-01-01T00:00:00Z",
			properties: {},
			tags: [],
			links: [],
		};
		seedEntry("search-store-ws", entry, record);
	});

	it("keyword search returns results", async () => {
		await createRoot(async (dispose) => {
			const store = createSearchStore(() => "search-store-ws");
			const results = await store.searchKeyword("rockets");
			expect(results.length).toBeGreaterThan(0);
			expect(store.results().length).toBeGreaterThan(0);
			expect(store.loading()).toBe(false);
			dispose();
		});
	});

	it("query index returns filtered results", async () => {
		await createRoot(async (dispose) => {
			const store = createSearchStore(() => "search-store-ws");
			const results = await store.queryIndex({});
			expect(Array.isArray(results)).toBe(true);
			expect(store.queryResults()).toBeDefined();
			dispose();
		});
	});

	it("sets error on keyword search failure", async () => {
		server.use(
			http.get(testApiUrl("/spaces/search-store-ws/search"), () =>
				HttpResponse.json({ detail: "Server error" }, { status: 500 }),
			),
		);
		await createRoot(async (dispose) => {
			const store = createSearchStore(() => "search-store-ws");
			await expect(store.searchKeyword("test")).rejects.toThrow();
			expect(store.error()).toContain("Failed to search entries");
			dispose();
		});
	});

	it("sets error on query index failure", async () => {
		server.use(
			http.post(testApiUrl("/spaces/search-store-ws/query"), () =>
				HttpResponse.json({ detail: "Server error" }, { status: 500 }),
			),
		);
		await createRoot(async (dispose) => {
			const store = createSearchStore(() => "search-store-ws");
			await expect(store.queryIndex({})).rejects.toThrow();
			expect(store.error()).toContain("Failed to query");
			dispose();
		});
	});
});
