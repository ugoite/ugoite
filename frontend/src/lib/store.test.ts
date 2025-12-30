import { describe, it, expect, beforeEach } from "vitest";
import { createRoot } from "solid-js";
import { createNoteStore } from "./store";
import { resetMockData, seedWorkspace, seedNote } from "~/test/mocks/handlers";
import type { Note, NoteRecord, Workspace } from "./types";

const testWorkspace: Workspace = {
	id: "store-test-ws",
	name: "Store Test Workspace",
	created_at: "2025-01-01T00:00:00Z",
};

describe("createNoteStore", () => {
	beforeEach(() => {
		resetMockData();
		seedWorkspace(testWorkspace);
	});

	it("should load notes from API", async () => {
		const note: Note = {
			id: "note-1",
			content: "# Test Note",
			revision_id: "rev-1",
			created_at: "2025-01-01T00:00:00Z",
			updated_at: "2025-01-01T00:00:00Z",
		};
		const record: NoteRecord = {
			id: "note-1",
			title: "Test Note",
			updated_at: "2025-01-01T00:00:00Z",
			properties: {},
			tags: [],
			links: [],
		};
		seedNote("store-test-ws", note, record);

		await createRoot(async (dispose) => {
			const store = createNoteStore(() => "store-test-ws");

			expect(store.notes()).toEqual([]);
			expect(store.loading()).toBe(false);

			await store.loadNotes();

			expect(store.notes()).toHaveLength(1);
			expect(store.notes()[0].title).toBe("Test Note");
			expect(store.loading()).toBe(false);

			dispose();
		});
	});

	it("should create a note and reload list", async () => {
		await createRoot(async (dispose) => {
			const store = createNoteStore(() => "store-test-ws");
			await store.loadNotes();

			expect(store.notes()).toHaveLength(0);

			const result = await store.createNote("# New Note\n\n## Status\nActive");

			expect(result.id).toBeDefined();
			expect(store.notes()).toHaveLength(1);
			expect(store.notes()[0].title).toBe("New Note");

			dispose();
		});
	});

	it("should apply optimistic updates during update", async () => {
		await createRoot(async (dispose) => {
			const store = createNoteStore(() => "store-test-ws");

			// Create a note first
			const createResult = await store.createNote("# Original Title");
			const noteId = createResult.id;

			// Get the note to have revision_id
			store.selectNote(noteId);

			// Wait for selected note to load
			await new Promise((resolve) => setTimeout(resolve, 50));

			const note = store.selectedNote();
			expect(note).not.toBeNull();

			// Update with optimistic behavior
			const updatePromise = store.updateNote(noteId, {
				markdown: "# Updated Title",
				parent_revision_id: note!.revision_id,
			});

			// Check optimistic state immediately
			const optimisticNote = store.notes().find((n) => n.id === noteId);
			expect(optimisticNote?.title).toBe("Updated Title");

			// Wait for server confirmation
			await updatePromise;

			// State should still be updated
			const confirmedNote = store.notes().find((n) => n.id === noteId);
			expect(confirmedNote?.title).toBe("Updated Title");

			dispose();
		});
	});

	it("should delete note optimistically", async () => {
		await createRoot(async (dispose) => {
			const store = createNoteStore(() => "store-test-ws");

			const createResult = await store.createNote("# To Delete");
			expect(store.notes()).toHaveLength(1);

			await store.deleteNote(createResult.id);

			expect(store.notes()).toHaveLength(0);

			dispose();
		});
	});

	it("should handle note selection", async () => {
		const note: Note = {
			id: "select-note",
			content: "# Selectable Note\n\nContent here",
			revision_id: "rev-select",
			created_at: "2025-01-01T00:00:00Z",
			updated_at: "2025-01-01T00:00:00Z",
		};
		const record: NoteRecord = {
			id: "select-note",
			title: "Selectable Note",
			updated_at: "2025-01-01T00:00:00Z",
			properties: {},
			tags: [],
			links: [],
		};
		seedNote("store-test-ws", note, record);

		await createRoot(async (dispose) => {
			const store = createNoteStore(() => "store-test-ws");
			await store.loadNotes();

			expect(store.selectedNoteId()).toBeNull();
			expect(store.selectedNote()).toBeNull();

			store.selectNote("select-note");
			expect(store.selectedNoteId()).toBe("select-note");

			// Wait for resource to load
			await new Promise((resolve) => setTimeout(resolve, 50));

			const selected = store.selectedNote();
			expect(selected?.content).toBe("# Selectable Note\n\nContent here");

			dispose();
		});
	});

	it("should clear selection when deleting selected note", async () => {
		await createRoot(async (dispose) => {
			const store = createNoteStore(() => "store-test-ws");

			const createResult = await store.createNote("# Selected Note");
			store.selectNote(createResult.id);
			expect(store.selectedNoteId()).toBe(createResult.id);

			await store.deleteNote(createResult.id);

			expect(store.selectedNoteId()).toBeNull();
			expect(store.notes()).toHaveLength(0);

			dispose();
		});
	});

	it("should set error state on failure", async () => {
		await createRoot(async (dispose) => {
			const store = createNoteStore(() => "non-existent-workspace");

			await store.loadNotes();

			expect(store.error()).not.toBeNull();

			dispose();
		});
	});
});
