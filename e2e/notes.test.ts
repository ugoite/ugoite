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

	test("GET /workspaces/default/notes returns note list", async ({ request }) => {
		const res = await request.get(
			getBackendUrl("/workspaces/default/notes"),
		);
		expect(res.ok()).toBeTruthy();

		const notes = await res.json();
		expect(Array.isArray(notes)).toBe(true);
	});

	test("consecutive PUT should succeed with updated revision_id", async ({ request }) => {
		const createRes = await request.post(
			getBackendUrl("/workspaces/default/notes"),
			{
				data: {
					content:
						"---\nclass: Note\n---\n# Initial Content\n\n## Body\nThis is the first version.",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string; revision_id: string };

		const firstUpdateRes = await request.put(
			getBackendUrl(`/workspaces/default/notes/${created.id}`),
			{
				data: {
					markdown:
						"---\nclass: Note\n---\n# Updated Content\n\n## Body\nThis is the second version.",
					parent_revision_id: created.revision_id,
				},
			},
		);
		expect(firstUpdateRes.ok()).toBeTruthy();
		const firstResult = (await firstUpdateRes.json()) as {
			revision_id: string;
		};

		const secondUpdateRes = await request.put(
			getBackendUrl(`/workspaces/default/notes/${created.id}`),
			{
				data: {
					markdown:
						"---\nclass: Note\n---\n# Third Version\n\n## Body\nThis is the third version.",
					parent_revision_id: firstResult.revision_id,
				},
			},
		);
		expect(secondUpdateRes.ok()).toBeTruthy();

		await request.delete(
			getBackendUrl(`/workspaces/default/notes/${created.id}`),
		);
	});

	test("PUT with stale revision_id should return 409 conflict", async ({ request }) => {
		const createRes = await request.post(
			getBackendUrl("/workspaces/default/notes"),
			{
				data: {
					content:
						"---\nclass: Note\n---\n# Conflict Test\n\n## Body\nTesting revision conflicts.",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string; revision_id: string };

		const firstUpdateRes = await request.put(
			getBackendUrl(`/workspaces/default/notes/${created.id}`),
			{
				data: {
					markdown:
						"---\nclass: Note\n---\n# After First Update\n\n## Body\nFirst update body",
					parent_revision_id: created.revision_id,
				},
			},
		);
		expect(firstUpdateRes.ok()).toBeTruthy();

		const conflictRes = await request.put(
			getBackendUrl(`/workspaces/default/notes/${created.id}`),
			{
				data: {
					markdown:
						"---\nclass: Note\n---\n# This Should Fail\n\n## Body\nStale revision",
					parent_revision_id: created.revision_id,
				},
			},
		);
		expect(conflictRes.status()).toBe(409);

		await request.delete(
			getBackendUrl(`/workspaces/default/notes/${created.id}`),
		);
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
