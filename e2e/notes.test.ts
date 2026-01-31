/**
 * Notes E2E Tests for IEapp
 *
 * These tests verify the full notes CRUD functionality:
 * - Create notes
 * - Update notes
 * - Delete notes
 */

import { expect, test } from "@playwright/test";
import { ensureDefaultClass, getBackendUrl, waitForServers } from "./lib/client";

test.describe("Notes CRUD", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
		await ensureDefaultClass(request);
	});

	test("POST /workspaces/default/notes creates a new note", async ({ request }) => {
		const timestamp = Date.now();
		const res = await request.post(
			getBackendUrl("/workspaces/default/notes"),
			{
				data: {
					content: `---\nclass: Note\n---\n# E2E Test Note ${timestamp}\n\n## Body\nCreated at ${new Date().toISOString()}`,
				},
			},
		);
		expect(res.status()).toBe(201);

		const note = (await res.json()) as { id: string };
		expect(note).toHaveProperty("id");

		await request.delete(getBackendUrl(`/workspaces/default/notes/${note.id}`));
	});

	test("PUT /workspaces/default/notes/:id updates note", async ({ request }) => {
		const createRes = await request.post(
			getBackendUrl("/workspaces/default/notes"),
			{
				data: {
					content:
						"---\nclass: Note\n---\n# Update Test Note\n\n## Body\nOriginal content",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		const getRes = await request.get(
			getBackendUrl(`/workspaces/default/notes/${created.id}`),
		);
		const current = (await getRes.json()) as { revision_id: string };

		const updateRes = await request.put(
			getBackendUrl(`/workspaces/default/notes/${created.id}`),
			{
				data: {
					markdown:
						"---\nclass: Note\n---\n# Updated Title\n\n## Body\nUpdated content by E2E test",
					parent_revision_id: current.revision_id,
				},
			},
		);
		expect(updateRes.ok()).toBeTruthy();

		await request.delete(getBackendUrl(`/workspaces/default/notes/${created.id}`));
	});

	test("DELETE /workspaces/default/notes/:id removes note", async ({ request }) => {
		const createRes = await request.post(
			getBackendUrl("/workspaces/default/notes"),
			{
				data: {
					content:
						"---\nclass: Note\n---\n# Delete Test Note\n\n## Body\nTo be deleted",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		const deleteRes = await request.delete(
			getBackendUrl(`/workspaces/default/notes/${created.id}`),
		);
		expect([200, 204]).toContain(deleteRes.status());

		const fetchRes = await request.get(
			getBackendUrl(`/workspaces/default/notes/${created.id}`),
		);
		expect(fetchRes.status()).toBe(404);
	});
});
