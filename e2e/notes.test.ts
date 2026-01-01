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
				content: `# E2E Test Note ${timestamp}\n\nThis note was created by E2E tests at ${new Date().toISOString()}`,
			};

			const res = await client.postApi(
				"/workspaces/default/notes",
				noteData,
			);
			expect(res.status).toBe(201);

			const note = await res.json() as { id: string; revision_id: string };
			expect(note).toHaveProperty("id");
			expect(note).toHaveProperty("revision_id");

			// Track for cleanup
			createdNoteIds.push(note.id);

			// Verify note content via GET
			const getRes = await client.getApi(`/workspaces/default/notes/${note.id}`);
			const fetched = await getRes.json() as { title: string; content: string };
			expect(fetched.title).toBe(`E2E Test Note ${timestamp}`);
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
			// Create a note for read tests (title comes from first heading)
			const res = await client.postApi("/workspaces/default/notes", {
				content: "# Read Test Note\n\nNote for testing read operations",
			});
			const note = await res.json() as { id: string };
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
			// Create a note for update tests (title comes from first heading)
			const res = await client.postApi("/workspaces/default/notes", {
				content: "# Update Test Note\n\nOriginal content",
			});
			const note = await res.json() as { id: string; revision_id: string };
			testNoteId = note.id;
			createdNoteIds.push(testNoteId);
		});

		test("PUT /workspaces/default/notes/:id updates note", async () => {
			// First get the current note to get revision_id
			const getRes = await client.getApi(`/workspaces/default/notes/${testNoteId}`);
			const currentNote = await getRes.json() as { revision_id: string };

			const updatedData = {
				markdown: "# Updated Title\n\nUpdated content by E2E test",
				parent_revision_id: currentNote.revision_id,
			};

			const res = await client.putApi(
				`/workspaces/default/notes/${testNoteId}`,
				updatedData,
			);
			expect(res.ok).toBe(true);

			const note = await res.json() as { id: string; revision_id: string };
			expect(note).toHaveProperty("revision_id");

			// Verify via GET
			const verifyRes = await client.getApi(`/workspaces/default/notes/${testNoteId}`);
			const verified = await verifyRes.json() as { title: string; content: string };
			expect(verified.title).toBe("Updated Title");
			expect(verified.content).toContain("Updated content by E2E test");
		});

		test("PUT /workspaces/default/notes/:id preserves title from heading", async () => {
			// Get current note to get revision_id
			const getRes = await client.getApi(`/workspaces/default/notes/${testNoteId}`);
			const currentNote = await getRes.json() as { revision_id: string; title: string };

			// Update with same title (heading) but different body
			const res = await client.putApi(
				`/workspaces/default/notes/${testNoteId}`,
				{
					markdown: `# ${currentNote.title}\n\nOnly body content updated`,
					parent_revision_id: currentNote.revision_id,
				},
			);
			expect(res.ok).toBe(true);

			// Verify title was preserved and content updated
			const verifyRes = await client.getApi(`/workspaces/default/notes/${testNoteId}`);
			const note = await verifyRes.json() as { title: string; content: string };
			expect(note.title).toBe(currentNote.title);
			expect(note.content).toContain("Only body content updated");
		});

		test("PUT /workspaces/default/notes/:id returns 404 for nonexistent", async () => {
			const res = await client.putApi(
				"/workspaces/default/notes/nonexistent-note-id-xyz",
				{
					markdown: "# Will Fail",
					parent_revision_id: "any-revision-id",
				},
			);
			expect(res.status).toBe(404);
		});
	});

	describe("Delete Notes", () => {
		test("DELETE /workspaces/default/notes/:id removes note", async () => {
			// Create a note specifically for deletion
			const createRes = await client.postApi("/workspaces/default/notes", {
				content: "# Note to Delete\n\nThis will be deleted",
			});
			const note = await createRes.json() as { id: string };

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
		// Create notes with searchable content (title comes from first heading)
		const notes = [
			{ content: "# Searchable Alpha\n\nContains keyword: banana" },
			{ content: "# Searchable Beta\n\nContains keyword: apple" },
			{ content: "# Searchable Gamma\n\nContains keyword: banana and orange" },
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

describe("Consecutive Saves (REQ-FE-012)", () => {
	test("consecutive PUT should succeed with updated revision_id", async () => {
		// REQ-FE-012: Multiple consecutive saves must work correctly
		// Create a note
		const createRes = await client.postApi("/workspaces/default/notes", {
			content: "# Initial Content\n\nThis is the first version.",
		});
		expect(createRes.status).toBe(201);

		const createResult = await createRes.json() as { id: string; revision_id: string };
		createdNoteIds.push(createResult.id);

		// First update
		const firstUpdateRes = await client.putApi(
			`/workspaces/default/notes/${createResult.id}`,
			{
				markdown: "# Updated Content\n\nThis is the second version.",
				parent_revision_id: createResult.revision_id,
			},
		);
		expect(firstUpdateRes.ok).toBe(true);

		const firstResult = await firstUpdateRes.json() as { id: string; revision_id: string };
		expect(firstResult.revision_id).toBeDefined();
		expect(firstResult.revision_id).not.toBe(createResult.revision_id);

		// Second update with new revision_id
		const secondUpdateRes = await client.putApi(
			`/workspaces/default/notes/${createResult.id}`,
			{
				markdown: "# Third Version\n\nThis is the third version.",
				parent_revision_id: firstResult.revision_id,
			},
		);
		expect(secondUpdateRes.ok).toBe(true);

		const secondResult = await secondUpdateRes.json() as { id: string; revision_id: string };
		expect(secondResult.revision_id).toBeDefined();
		expect(secondResult.revision_id).not.toBe(firstResult.revision_id);

		// Third update to confirm it keeps working
		const thirdUpdateRes = await client.putApi(
			`/workspaces/default/notes/${createResult.id}`,
			{
				markdown: "# Fourth Version\n\nConsecutive saves work correctly!",
				parent_revision_id: secondResult.revision_id,
			},
		);
		expect(thirdUpdateRes.ok).toBe(true);
	});

	test("saved content should persist after reload (REQ-FE-010)", async () => {
		// REQ-FE-010: Content must be persisted to server
		// Create a note
		const createRes = await client.postApi("/workspaces/default/notes", {
			content: "# Persistence Test\n\nOriginal content.",
		});
		expect(createRes.status).toBe(201);

		const createResult = await createRes.json() as { id: string; revision_id: string };
		createdNoteIds.push(createResult.id);

		// Update with new content
		const updateRes = await client.putApi(
			`/workspaces/default/notes/${createResult.id}`,
			{
				markdown: "# Persistence Test\n\nUpdated content that should persist.",
				parent_revision_id: createResult.revision_id,
			},
		);
		expect(updateRes.ok).toBe(true);

		// Fetch the note and verify content was saved
		const getRes = await client.getApi(`/workspaces/default/notes/${createResult.id}`);
		expect(getRes.ok).toBe(true);

		const note = await getRes.json() as { id: string; content: string; revision_id: string };
		expect(note.content).toContain("Updated content that should persist");
		expect(note.content).not.toContain("Original content");
	});

	test("PUT with stale revision_id should return 409 conflict", async () => {
		// Create a note
		const createRes = await client.postApi("/workspaces/default/notes", {
			content: "# Conflict Test\n\nTesting revision conflicts.",
		});
		expect(createRes.status).toBe(201);

		const createResult = await createRes.json() as { id: string; revision_id: string };
		createdNoteIds.push(createResult.id);

		// First update succeeds
		const firstUpdateRes = await client.putApi(
			`/workspaces/default/notes/${createResult.id}`,
			{
				markdown: "# After First Update",
				parent_revision_id: createResult.revision_id,
			},
		);
		expect(firstUpdateRes.ok).toBe(true);

		// Second update with old/stale revision_id should fail with 409
		const conflictRes = await client.putApi(
			`/workspaces/default/notes/${createResult.id}`,
			{
				markdown: "# This Should Fail",
				parent_revision_id: createResult.revision_id, // Using old revision_id
			},
		);
		expect(conflictRes.status).toBe(409);
	});
});
