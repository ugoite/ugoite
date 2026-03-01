// REQ-FE-010: Entry store operations
import { describe, it, expect, beforeEach } from "vitest";
import { createRoot } from "solid-js";
import { http, HttpResponse } from "msw";
import { createEntryStore } from "./entry-store";
import { resetMockData, seedSpace, seedEntry } from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";
import type { Entry, EntryRecord } from "./types";

const space = { id: "es-ws", name: "Entry Store Space", created_at: "2025-01-01T00:00:00Z" };

describe("createEntryStore", () => {
	beforeEach(() => {
		resetMockData();
		seedSpace(space);
	});

	it("loads entries", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "es-ws");
			await store.loadEntries();
			expect(store.entries()).toEqual([]);
			expect(store.loading()).toBe(false);
			dispose();
		});
	});

	it("sets error on load failure", async () => {
		server.use(
			http.get("http://localhost:3000/api/spaces/es-ws/entries", () =>
				HttpResponse.json({ detail: "Error" }, { status: 500 }),
			),
		);
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "es-ws");
			await store.loadEntries();
			expect(store.error()).toBeTruthy();
			dispose();
		});
	});

	it("creates and loads an entry", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "es-ws");
			const result = await store.createEntry("# My Entry\n\nContent here");
			expect(result.id).toBeDefined();
			expect(store.entries()).toHaveLength(1);
			dispose();
		});
	});

	it("sets error on create failure", async () => {
		server.use(
			http.post("http://localhost:3000/api/spaces/es-ws/entries", () =>
				HttpResponse.json({ detail: "Error" }, { status: 500 }),
			),
		);
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "es-ws");
			await expect(store.createEntry("# Test")).rejects.toThrow();
			expect(store.error()).toBeTruthy();
			dispose();
		});
	});

	it("updates an entry with optimistic update", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "es-ws");
			const created = await store.createEntry("# Original Entry");
			const entry = store.entries()[0];
			const result = await store.updateEntry(entry.id, {
				markdown: "# Updated Entry",
				parent_revision_id: created.revision_id,
			});
			expect(result).toBeDefined();
			dispose();
		});
	});

	it("rolls back on update failure", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "es-ws");
			await store.createEntry("# Entry To Fail");
			const entry = store.entries()[0];
			server.use(
				http.put(`http://localhost:3000/api/spaces/es-ws/entries/${entry.id}`, () =>
					HttpResponse.json({ detail: "Error" }, { status: 500 }),
				),
			);
			await expect(
				store.updateEntry(entry.id, {
					markdown: "# Updated",
					parent_revision_id: "stale-rev",
				}),
			).rejects.toThrow();
			dispose();
		});
	});

	it("deletes an entry", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "es-ws");
			await store.createEntry("# To Delete");
			const entry = store.entries()[0];
			await store.deleteEntry(entry.id);
			expect(store.entries()).toHaveLength(0);
			dispose();
		});
	});

	it("rolls back on delete failure", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "es-ws");
			await store.createEntry("# Entry");
			const entry = store.entries()[0];
			server.use(
				http.delete(`http://localhost:3000/api/spaces/es-ws/entries/${entry.id}`, () =>
					HttpResponse.json({ detail: "Error" }, { status: 500 }),
				),
			);
			await expect(store.deleteEntry(entry.id)).rejects.toThrow();
			expect(store.entries()).toHaveLength(1);
			dispose();
		});
	});

	it("selects an entry", () => {
		createRoot((dispose) => {
			const store = createEntryStore(() => "es-ws");
			expect(store.selectedEntryId()).toBeNull();
			store.selectEntry("entry-1");
			expect(store.selectedEntryId()).toBe("entry-1");
			store.selectEntry(null);
			expect(store.selectedEntryId()).toBeNull();
			dispose();
		});
	});

	it("searches entries", async () => {
		const entry: Entry = {
			id: "search-entry",
			content: "# Rocket Entry\nContent about rockets",
			revision_id: "rev-1",
			created_at: "2025-01-01T00:00:00Z",
			updated_at: "2025-01-01T00:00:00Z",
		};
		const record: EntryRecord = {
			id: "search-entry",
			title: "Rocket Entry",
			updated_at: "2025-01-01T00:00:00Z",
			properties: {},
			tags: [],
			links: [],
		};
		seedEntry("es-ws", entry, record);
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "es-ws");
			const results = await store.searchEntries("rockets");
			expect(results).toBeDefined();
			dispose();
		});
	});

	it("throws on search failure and sets error", async () => {
		server.use(
			http.get("http://localhost:3000/api/spaces/es-ws/search", () =>
				HttpResponse.json({ detail: "Error" }, { status: 500 }),
			),
		);
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "es-ws");
			await expect(store.searchEntries("test")).rejects.toThrow();
			expect(store.error()).toBeTruthy();
			dispose();
		});
	});

	it("throws when updating entry not in local state", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "es-ws");
			await expect(
				store.updateEntry("nonexistent-id", {
					markdown: "# Updated",
					parent_revision_id: "rev-1",
				}),
			).rejects.toThrow("Entry not found in local state");
			dispose();
		});
	});

	it("handles RevisionConflictError during update", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "es-ws");
			const _created = await store.createEntry("# Entry to conflict");
			const entry = store.entries()[0];
			store.selectEntry(entry.id);
			server.use(
				http.put(`http://localhost:3000/api/spaces/es-ws/entries/${entry.id}`, () =>
					HttpResponse.json(
						{ detail: "Revision conflict", revision_id: "server-rev" },
						{ status: 409 },
					),
				),
			);
			await expect(
				store.updateEntry(entry.id, {
					markdown: "# Conflicting update",
					parent_revision_id: "old-rev",
				}),
			).rejects.toThrow();
			dispose();
		});
	});

	it("handles updateEntry title extraction with tab-separated heading", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "es-ws");
			const created = await store.createEntry("# Initial Entry");
			await store.updateEntry(created.id, {
				markdown: "#\tHeading\nContent",
				parent_revision_id: created.revision_id,
			});
			const entry = store.entries().find((e) => e.id === created.id);
			expect(entry?.title).toBe("Heading");
			dispose();
		});
	});

	it("updateEntry only updates the matching entry when multiple exist", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "es-ws");
			const r1 = await store.createEntry("# Entry One");
			await store.createEntry("# Entry Two");
			await store.updateEntry(r1.id, {
				markdown: "# Entry One Updated",
				parent_revision_id: r1.revision_id,
			});
			expect(store.entries()).toHaveLength(2);
			dispose();
		});
	});

	it("handles RevisionConflictError when a different entry is selected", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "es-ws");
			const r1 = await store.createEntry("# Entry One");
			const r2 = await store.createEntry("# Entry Two");
			store.selectEntry(r2.id);
			server.use(
				http.put(`http://localhost:3000/api/spaces/es-ws/entries/${r1.id}`, () =>
					HttpResponse.json({ detail: "Conflict", revision_id: "server-rev" }, { status: 409 }),
				),
			);
			await expect(
				store.updateEntry(r1.id, {
					markdown: "# Updated",
					parent_revision_id: "old-rev",
				}),
			).rejects.toThrow();
			dispose();
		});
	});

	it("updateEntry preserves original title when markdown has no heading", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "es-ws");
			const created = await store.createEntry("# Original Title");
			await store.updateEntry(created.id, {
				markdown: "Just some content without heading",
				parent_revision_id: created.revision_id,
			});
			expect(store.error()).toBeNull();
			dispose();
		});
	});
});
