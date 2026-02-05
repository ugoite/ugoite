import { describe, it, expect, beforeEach } from "vitest";
import { createRoot, createEffect } from "solid-js";
import { createEntryStore } from "./entry-store";
import { resetMockData, seedSpace, seedEntry } from "~/test/mocks/handlers";
import type { Entry, EntryRecord, Space } from "./types";

const testSpace: Space = {
	id: "store-test-ws",
	name: "Store Test Space",
	created_at: "2025-01-01T00:00:00Z",
};

describe("createEntryStore", () => {
	beforeEach(() => {
		resetMockData();
		seedSpace(testSpace);
	});

	it("should load entries from API", async () => {
		const entry: Entry = {
			id: "entry-1",
			content: "# Test Entry",
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
		seedEntry("store-test-ws", entry, record);

		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "store-test-ws");

			expect(store.entries()).toEqual([]);
			expect(store.loading()).toBe(false);

			await store.loadEntries();

			expect(store.entries()).toHaveLength(1);
			expect(store.entries()[0].title).toBe("Test Entry");
			expect(store.loading()).toBe(false);

			dispose();
		});
	});

	it("should create a entry and reload list", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "store-test-ws");
			await store.loadEntries();

			expect(store.entries()).toHaveLength(0);

			const result = await store.createEntry("# New Entry\n\n## Status\nActive");

			expect(result.id).toBeDefined();
			expect(store.entries()).toHaveLength(1);
			expect(store.entries()[0].title).toBe("New Entry");

			dispose();
		});
	});

	it("should apply optimistic updates during update", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "store-test-ws");

			// Create a entry first
			const createResult = await store.createEntry("# Original Title");
			const entryId = createResult.id;

			// Get the entry to have revision_id
			store.selectEntry(entryId);

			// Wait for selected entry to load
			await new Promise((resolve) => setTimeout(resolve, 50));

			const entry = store.selectedEntry();
			expect(entry).not.toBeNull();

			if (!entry) {
				throw new Error("Entry should be loaded");
			}

			// Update with optimistic behavior
			const updatePromise = store.updateEntry(entryId, {
				markdown: "# Updated Title",
				parent_revision_id: entry.revision_id,
			});

			// Check optimistic state immediately
			const optimisticEntry = store.entries().find((n) => n.id === entryId);
			expect(optimisticEntry?.title).toBe("Updated Title");

			// Wait for server confirmation
			await updatePromise;

			// State should still be updated
			const confirmedEntry = store.entries().find((n) => n.id === entryId);
			expect(confirmedEntry?.title).toBe("Updated Title");

			dispose();
		});
	});

	it("should delete entry optimistically", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "store-test-ws");

			const createResult = await store.createEntry("# To Delete");
			expect(store.entries()).toHaveLength(1);

			await store.deleteEntry(createResult.id);

			expect(store.entries()).toHaveLength(0);

			dispose();
		});
	});

	it("should handle entry selection", async () => {
		const entry: Entry = {
			id: "select-entry",
			content: "# Selectable Entry\n\nContent here",
			revision_id: "rev-select",
			created_at: "2025-01-01T00:00:00Z",
			updated_at: "2025-01-01T00:00:00Z",
		};
		const record: EntryRecord = {
			id: "select-entry",
			title: "Selectable Entry",
			updated_at: "2025-01-01T00:00:00Z",
			properties: {},
			tags: [],
			links: [],
		};
		seedEntry("store-test-ws", entry, record);

		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "store-test-ws");
			await store.loadEntries();

			expect(store.selectedEntryId()).toBeNull();
			// createResource returns undefined when source is null (no entry selected)
			expect(store.selectedEntry()).toBeUndefined();

			store.selectEntry("select-entry");
			expect(store.selectedEntryId()).toBe("select-entry");

			// Wait for resource to load
			await new Promise((resolve) => setTimeout(resolve, 50));

			const selected = store.selectedEntry();
			expect(selected?.content).toBe("# Selectable Entry\n\nContent here");

			dispose();
		});
	});

	it("should clear selection when deleting selected entry", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "store-test-ws");

			const createResult = await store.createEntry("# Selected Entry");
			store.selectEntry(createResult.id);
			expect(store.selectedEntryId()).toBe(createResult.id);

			await store.deleteEntry(createResult.id);

			expect(store.selectedEntryId()).toBeNull();
			expect(store.entries()).toHaveLength(0);

			dispose();
		});
	});

	it("should set error state on failure", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "non-existent-space");

			await store.loadEntries();

			expect(store.error()).not.toBeNull();

			dispose();
		});
	});

	it("should not refetch after successful update", async () => {
		// REQ-FE-010: Editor content MUST NOT be overwritten during or after save operation
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "store-test-ws");

			// Create a entry
			const createResult = await store.createEntry("# Original");
			const entryId = createResult.id;

			// Select and load the entry
			store.selectEntry(entryId);
			await new Promise((resolve) => setTimeout(resolve, 50));

			const entry = store.selectedEntry();
			expect(entry).not.toBeNull();
			if (!entry) throw new Error("Entry should be loaded");

			// Count how many times selectedEntry changes
			let loadCount = 0;
			const _unsubscribe = createRoot(() => {
				createEffect(() => {
					store.selectedEntry();
					loadCount++;
				});
			});

			const initialLoadCount = loadCount;

			// Update the entry
			await store.updateEntry(entryId, {
				markdown: "# Updated",
				parent_revision_id: entry.revision_id,
			});

			// Wait a bit to ensure no async refetch happens
			await new Promise((resolve) => setTimeout(resolve, 100));

			// selectedEntry should NOT have been refetched (loadCount should not increase significantly)
			// The initial load is 1, and we expect no additional loads from refetch
			expect(loadCount).toBeLessThanOrEqual(initialLoadCount + 1);

			dispose();
		});
	});

	it("should support consecutive saves with updated revision_id", async () => {
		// REQ-FE-012: Multiple consecutive saves must work correctly
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "store-test-ws");

			// Create a entry
			const createResult = await store.createEntry("# First Version");
			const entryId = createResult.id;

			// Select and load the entry
			store.selectEntry(entryId);
			await new Promise((resolve) => setTimeout(resolve, 50));

			const entry = store.selectedEntry();
			expect(entry).not.toBeNull();
			if (!entry) throw new Error("Entry should be loaded");

			// First update
			const firstResult = await store.updateEntry(entryId, {
				markdown: "# Second Version",
				parent_revision_id: entry.revision_id,
			});

			expect(firstResult.revision_id).toBeDefined();
			expect(firstResult.revision_id).not.toBe(entry.revision_id);

			// Second update using the new revision_id
			const secondResult = await store.updateEntry(entryId, {
				markdown: "# Third Version",
				parent_revision_id: firstResult.revision_id,
			});

			expect(secondResult.revision_id).toBeDefined();
			expect(secondResult.revision_id).not.toBe(firstResult.revision_id);

			// Third update to confirm it keeps working
			const thirdResult = await store.updateEntry(entryId, {
				markdown: "# Fourth Version",
				parent_revision_id: secondResult.revision_id,
			});

			expect(thirdResult.revision_id).toBeDefined();
			expect(thirdResult.revision_id).not.toBe(secondResult.revision_id);

			dispose();
		});
	});

	it("should search entries by keyword via store helper", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "store-test-ws");
			await store.createEntry("# Searchable Entry\nDetails here");
			const results = await store.searchEntries("Searchable");
			expect(results.length).toBeGreaterThan(0);
			dispose();
		});
	});

	it("propagates assets in optimistic update", async () => {
		await createRoot(async (dispose) => {
			const store = createEntryStore(() => "store-test-ws");

			const createResult = await store.createEntry("# Asset Entry");
			const asset = { id: "att-1", name: "voice.m4a", path: "assets/att-1_voice.m4a" };

			await store.updateEntry(createResult.id, {
				markdown: "# Asset Entry\nupdated",
				parent_revision_id: createResult.revision_id,
				assets: [asset],
			});

			const updated = store.entries().find((n) => n.id === createResult.id);
			expect(updated?.assets?.[0]?.id).toBe("att-1");

			dispose();
		});
	});
});
