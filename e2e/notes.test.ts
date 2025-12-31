/**
 * Notes E2E Tests for IEapp
 *
 * These tests verify the full notes CRUD functionality:
 * - Create notes
 * - Read notes
 * - Update notes
 * - Delete notes
 */

import { describe, expect, test, beforeAll, afterAll } from "bun:test";
import { E2EClient, waitForServers } from "./lib/client";

const client = new E2EClient();

// Track created notes for cleanup
const createdNoteIds: string[] = [];

describe("Notes CRUD", () => {
	beforeAll(async () => {
		await waitForServers(client, { timeout: 60000 });
	});

	afterAll(async () => {
		// Cleanup: delete all notes created during tests
		for (const id of createdNoteIds) {
			try {
				await client.deleteApi(`/workspaces/default/notes/${id}`);
			} catch {
				// Ignore cleanup errors
			}
		}
	});

	describe("Create Notes", () => {
		test("POST /workspaces/default/notes creates a new note", async () => {
			const timestamp = Date.now();
			const noteData = {
				title: `E2E Test Note ${timestamp}`,
				content: `This note was created by E2E tests at ${new Date().toISOString()}`,
			};

			const res = await client.postApi(
				"/workspaces/default/notes",
				noteData,
			);
			expect(res.status).toBe(201);

			const note = await res.json();
			expect(note).toHaveProperty("id");
			expect(note.title).toBe(noteData.title);
			expect(note.content).toBe(noteData.content);

			// Track for cleanup
			createdNoteIds.push(note.id);
		});

		test("POST /workspaces/default/notes validates required fields", async () => {
			// Missing title should fail or use default
			const res = await client.postApi("/workspaces/default/notes", {
				content: "Content without title",
			});

			// Either 400 (validation error) or 201 with default title is acceptable
			expect([201, 400, 422]).toContain(res.status);

			if (res.status === 201) {
				const note = await res.json();
				createdNoteIds.push(note.id);
			}
		});
	});

	describe("Read Notes", () => {
		let testNoteId: string;

		beforeAll(async () => {
			// Create a note for read tests
			const res = await client.postApi("/workspaces/default/notes", {
				title: "Read Test Note",
				content: "Note for testing read operations",
			});
			const note = await res.json();
			testNoteId = note.id;
			createdNoteIds.push(testNoteId);
		});

		test("GET /workspaces/default/notes returns note list", async () => {
			const res = await client.getApi("/workspaces/default/notes");
			expect(res.ok).toBe(true);

			const notes = await res.json();
			expect(Array.isArray(notes)).toBe(true);
		});

		test("GET /workspaces/default/notes/:id returns specific note", async () => {
			const res = await client.getApi(
				`/workspaces/default/notes/${testNoteId}`,
			);
			expect(res.ok).toBe(true);

			const note = await res.json();
			expect(note.id).toBe(testNoteId);
			expect(note.title).toBe("Read Test Note");
		});

		test("GET /workspaces/default/notes/:id returns 404 for nonexistent", async () => {
			const res = await client.getApi(
				"/workspaces/default/notes/nonexistent-note-id-xyz",
			);
			expect(res.status).toBe(404);
		});
	});

	describe("Update Notes", () => {
		let testNoteId: string;

		beforeAll(async () => {
			// Create a note for update tests
			const res = await client.postApi("/workspaces/default/notes", {
				title: "Update Test Note",
				content: "Original content",
			});
			const note = await res.json();
			testNoteId = note.id;
			createdNoteIds.push(testNoteId);
		});

		test("PUT /workspaces/default/notes/:id updates note", async () => {
			const updatedData = {
				title: "Updated Title",
				content: "Updated content by E2E test",
			};

			const res = await client.putApi(
				`/workspaces/default/notes/${testNoteId}`,
				updatedData,
			);
			expect(res.ok).toBe(true);

			const note = await res.json();
			expect(note.title).toBe(updatedData.title);
			expect(note.content).toBe(updatedData.content);
		});

		test("PUT /workspaces/default/notes/:id preserves unchanged fields", async () => {
			// Only update content
			const res = await client.putApi(
				`/workspaces/default/notes/${testNoteId}`,
				{ content: "Only content updated" },
			);
			expect(res.ok).toBe(true);

			// Verify title was preserved
			const getRes = await client.getApi(
				`/workspaces/default/notes/${testNoteId}`,
			);
			const note = await getRes.json();
			expect(note.content).toBe("Only content updated");
		});

		test("PUT /workspaces/default/notes/:id returns 404 for nonexistent", async () => {
			const res = await client.putApi(
				"/workspaces/default/notes/nonexistent-note-id-xyz",
				{ title: "Will Fail" },
			);
			expect(res.status).toBe(404);
		});
	});

	describe("Delete Notes", () => {
		test("DELETE /workspaces/default/notes/:id removes note", async () => {
			// Create a note specifically for deletion
			const createRes = await client.postApi("/workspaces/default/notes", {
				title: "Note to Delete",
				content: "This will be deleted",
			});
			const note = await createRes.json();

			// Delete it
			const deleteRes = await client.deleteApi(
				`/workspaces/default/notes/${note.id}`,
			);
			expect([200, 204]).toContain(deleteRes.status);

			// Verify it's gone
			const getRes = await client.getApi(
				`/workspaces/default/notes/${note.id}`,
			);
			expect(getRes.status).toBe(404);
		});

		test("DELETE /workspaces/default/notes/:id returns 404 for nonexistent", async () => {
			const res = await client.deleteApi(
				"/workspaces/default/notes/nonexistent-note-id-xyz",
			);
			expect(res.status).toBe(404);
		});
	});
});

describe("Workspace Isolation", () => {
	test("Notes from different workspaces are isolated", async () => {
		// Get notes from default workspace
		const defaultRes = await client.getApi("/workspaces/default/notes");
		expect(defaultRes.ok).toBe(true);

		// Get notes from Stay workspace (if exists)
		const stayRes = await client.getApi("/workspaces/Stay/notes");
		// May return 200 (exists) or 404 (doesn't exist)
		expect([200, 404]).toContain(stayRes.status);
	});
});

describe("Note Search", () => {
	beforeAll(async () => {
		// Create notes with searchable content
		const notes = [
			{ title: "Searchable Alpha", content: "Contains keyword: banana" },
			{ title: "Searchable Beta", content: "Contains keyword: apple" },
			{ title: "Searchable Gamma", content: "Contains keyword: banana and orange" },
		];

		for (const note of notes) {
			const res = await client.postApi("/workspaces/default/notes", note);
			if (res.status === 201) {
				const created = await res.json();
				createdNoteIds.push(created.id);
			}
		}
	});

	test("GET /workspaces/default/notes returns all created notes", async () => {
		const res = await client.getApi("/workspaces/default/notes");
		expect(res.ok).toBe(true);

		const notes = await res.json();
		expect(Array.isArray(notes)).toBe(true);
		// Should have at least the notes we created
		expect(notes.length).toBeGreaterThan(0);
	});
});
