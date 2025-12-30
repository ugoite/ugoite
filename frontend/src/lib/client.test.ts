import { describe, it, expect, beforeEach } from "vitest";
import { noteApi, workspaceApi, RevisionConflictError } from "./client";
import { resetMockData, seedWorkspace, seedNote } from "~/test/mocks/handlers";
import type { Note, NoteRecord, Workspace } from "./types";

describe("workspaceApi", () => {
	beforeEach(() => {
		resetMockData();
	});

	describe("list", () => {
		it("should return empty array when no workspaces exist", async () => {
			const workspaces = await workspaceApi.list();
			expect(workspaces).toEqual([]);
		});

		it("should return all workspaces", async () => {
			const ws1: Workspace = { id: "ws1", name: "Workspace 1", created_at: "2025-01-01T00:00:00Z" };
			const ws2: Workspace = { id: "ws2", name: "Workspace 2", created_at: "2025-01-02T00:00:00Z" };
			seedWorkspace(ws1);
			seedWorkspace(ws2);

			const workspaces = await workspaceApi.list();
			expect(workspaces).toHaveLength(2);
			expect(workspaces.map((w) => w.id)).toContain("ws1");
			expect(workspaces.map((w) => w.id)).toContain("ws2");
		});
	});

	describe("create", () => {
		it("should create a new workspace", async () => {
			const result = await workspaceApi.create("my-workspace");
			expect(result.id).toBe("my-workspace");
			expect(result.name).toBe("my-workspace");

			// Verify it exists
			const workspaces = await workspaceApi.list();
			expect(workspaces).toHaveLength(1);
		});

		it("should throw error for duplicate workspace", async () => {
			await workspaceApi.create("my-workspace");
			await expect(workspaceApi.create("my-workspace")).rejects.toThrow("already exists");
		});
	});
});

describe("noteApi", () => {
	const testWorkspace: Workspace = {
		id: "test-ws",
		name: "Test Workspace",
		created_at: "2025-01-01T00:00:00Z",
	};

	beforeEach(() => {
		resetMockData();
		seedWorkspace(testWorkspace);
	});

	describe("list", () => {
		it("should return empty array when no notes exist", async () => {
			const notes = await noteApi.list("test-ws");
			expect(notes).toEqual([]);
		});

		it("should return all notes in workspace", async () => {
			const note: Note = {
				id: "note-1",
				content: "# Test Note\n\nContent",
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
			seedNote("test-ws", note, record);

			const notes = await noteApi.list("test-ws");
			expect(notes).toHaveLength(1);
			expect(notes[0].title).toBe("Test Note");
		});
	});

	describe("create", () => {
		it("should create a note and extract title from markdown", async () => {
			const result = await noteApi.create("test-ws", {
				content: "# My Meeting Notes\n\n## Date\n2025-01-15\n\n## Attendees\nAlice, Bob",
			});

			expect(result.id).toBeDefined();
			expect(result.revision_id).toBeDefined();

			// Verify the note was indexed with extracted properties
			const notes = await noteApi.list("test-ws");
			expect(notes).toHaveLength(1);
			expect(notes[0].title).toBe("My Meeting Notes");
			expect(notes[0].properties).toHaveProperty("Date");
			expect(notes[0].properties).toHaveProperty("Attendees");
		});

		it("should extract H2 headers as properties", async () => {
			const result = await noteApi.create("test-ws", {
				content: "# Task\n\n## Status\nPending\n\n## Priority\nHigh",
			});

			const notes = await noteApi.list("test-ws");
			const note = notes.find((n) => n.id === result.id);
			expect(note?.properties.Status).toBe("Pending");
			expect(note?.properties.Priority).toBe("High");
		});
	});

	describe("get", () => {
		it("should return full note content", async () => {
			const content = "# Full Note\n\nWith body content";
			const note: Note = {
				id: "note-get",
				content,
				revision_id: "rev-get",
				created_at: "2025-01-01T00:00:00Z",
				updated_at: "2025-01-01T00:00:00Z",
			};
			const record: NoteRecord = {
				id: "note-get",
				title: "Full Note",
				updated_at: "2025-01-01T00:00:00Z",
				properties: {},
				tags: [],
				links: [],
			};
			seedNote("test-ws", note, record);

			const fetched = await noteApi.get("test-ws", "note-get");
			expect(fetched.content).toBe(content);
			expect(fetched.revision_id).toBe("rev-get");
		});

		it("should throw error for non-existent note", async () => {
			await expect(noteApi.get("test-ws", "non-existent")).rejects.toThrow();
		});
	});

	describe("update", () => {
		it("should update note with correct parent_revision_id", async () => {
			const createResult = await noteApi.create("test-ws", {
				content: "# Original\n\n## Status\nDraft",
			});

			const updateResult = await noteApi.update("test-ws", createResult.id, {
				markdown: "# Updated\n\n## Status\nPublished",
				parent_revision_id: createResult.revision_id,
			});

			expect(updateResult.revision_id).not.toBe(createResult.revision_id);

			// Verify index was updated
			const notes = await noteApi.list("test-ws");
			const note = notes.find((n) => n.id === createResult.id);
			expect(note?.title).toBe("Updated");
			expect(note?.properties.Status).toBe("Published");
		});

		it("should throw RevisionConflictError (409) on revision mismatch", async () => {
			const createResult = await noteApi.create("test-ws", {
				content: "# Original",
			});

			// First update succeeds
			await noteApi.update("test-ws", createResult.id, {
				markdown: "# First Update",
				parent_revision_id: createResult.revision_id,
			});

			// Second update with stale revision should fail
			await expect(
				noteApi.update("test-ws", createResult.id, {
					markdown: "# Stale Update",
					parent_revision_id: createResult.revision_id, // Stale!
				}),
			).rejects.toThrow(RevisionConflictError);
		});
	});

	describe("delete", () => {
		it("should remove note from list", async () => {
			const result = await noteApi.create("test-ws", {
				content: "# To Delete",
			});

			let notes = await noteApi.list("test-ws");
			expect(notes).toHaveLength(1);

			await noteApi.delete("test-ws", result.id);

			notes = await noteApi.list("test-ws");
			expect(notes).toHaveLength(0);
		});
	});
});
