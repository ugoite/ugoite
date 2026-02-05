/**
 * Entries E2E Tests for IEapp
 *
 * These tests verify the full entries CRUD functionality:
 * - Create entries
 * - Update entries
 * - Delete entries
 */

import { expect, test } from "@playwright/test";
import { ensureDefaultForm, getBackendUrl, waitForServers } from "./lib/client";

test.describe("Entries CRUD", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
		await ensureDefaultForm(request);
	});

	test("POST /spaces/default/entries creates a new entry", async ({ request }) => {
		const timestamp = Date.now();
		const res = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					content: `---\nform: Entry\n---\n# E2E Test Entry ${timestamp}\n\n## Body\nCreated at ${new Date().toISOString()}`,
				},
			},
		);
		expect(res.status()).toBe(201);

		const entry = (await res.json()) as { id: string };
		expect(entry).toHaveProperty("id");

		await request.delete(getBackendUrl(`/spaces/default/entries/${entry.id}`));
	});

	test("GET /spaces/default/entries returns entry list", async ({ request }) => {
		const res = await request.get(
			getBackendUrl("/spaces/default/entries"),
		);
		expect(res.ok()).toBeTruthy();

		const entries = await res.json();
		expect(Array.isArray(entries)).toBe(true);
	});

	test("consecutive PUT should succeed with updated revision_id", async ({ request }) => {
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					content:
						"---\nform: Entry\n---\n# Initial Content\n\n## Body\nThis is the first version.",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string; revision_id: string };

		const firstUpdateRes = await request.put(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# Updated Content\n\n## Body\nThis is the second version.",
					parent_revision_id: created.revision_id,
				},
			},
		);
		expect(firstUpdateRes.ok()).toBeTruthy();
		const firstResult = (await firstUpdateRes.json()) as {
			revision_id: string;
		};

		const secondUpdateRes = await request.put(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# Third Version\n\n## Body\nThis is the third version.",
					parent_revision_id: firstResult.revision_id,
				},
			},
		);
		expect(secondUpdateRes.ok()).toBeTruthy();

		await request.delete(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
	});

	test("PUT with stale revision_id should return 409 conflict", async ({ request }) => {
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					content:
						"---\nform: Entry\n---\n# Conflict Test\n\n## Body\nTesting revision conflicts.",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string; revision_id: string };

		const firstUpdateRes = await request.put(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# After First Update\n\n## Body\nFirst update body",
					parent_revision_id: created.revision_id,
				},
			},
		);
		expect(firstUpdateRes.ok()).toBeTruthy();

		const conflictRes = await request.put(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# This Should Fail\n\n## Body\nStale revision",
					parent_revision_id: created.revision_id,
				},
			},
		);
		expect(conflictRes.status()).toBe(409);

		await request.delete(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
	});

	test("saved content should persist after reload (REQ-FE-010)", async ({ page, request }) => {
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					content:
						"---\nform: Entry\n---\n# Persistence Test\n\n## Body\nOriginal content.",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string; revision_id: string };

		const updateRes = await request.put(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# Persistence Test\n\n## Body\nUpdated content that should persist.",
					parent_revision_id: created.revision_id,
				},
			},
		);
		expect(updateRes.ok()).toBeTruthy();

		await page.goto(`/spaces/default/entries/${created.id}`);
		await page.waitForLoadState("networkidle");
		const html = await page.content();
		expect(html).toContain("Updated content that should persist");
		expect(html).not.toContain("Original content");

		await request.delete(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
	});

	test("REQ-FE-033: frontend entry detail route renders (not SolidJS Not Found)", async ({ page, request }) => {
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					content:
						"---\nform: Entry\n---\n# Detail Route Test\n\n## Body\nRoute render check.",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		await page.goto(`/spaces/default/entries/${created.id}`);
		await page.waitForLoadState("networkidle");
		const html = await page.content();
		expect(html).not.toContain("Visit solidjs.com");
		expect(html).not.toContain("NOT FOUND");

		await request.delete(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
	});

	test("REQ-FE-033: Retrieve entry with special characters", async ({ page, request }) => {
		const timestamp = Date.now();
		const title = `Special Entry @ ${timestamp} % &`;
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					content: `---\nform: Entry\n---\n# ${title}\n\n## Body\nTesting special chars in title.`,
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		await page.goto(`/spaces/default/entries/${encodeURIComponent(created.id)}`);
		await page.waitForLoadState("networkidle");
		const html = await page.content();
		expect(html).toContain(title);

		await request.delete(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
	});

	test("REQ-FE-034: Multi-entry navigation should not get stuck in loading state", async ({ page, request }) => {
		const formEntries = await Promise.all([
			request.post(getBackendUrl("/spaces/default/entries"), {
				data: {
					content: "---\nform: Entry\n---\n# Entry A\n\n## Body\nContent A",
				},
			}),
			request.post(getBackendUrl("/spaces/default/entries"), {
				data: {
					content: "---\nform: Entry\n---\n# Entry B\n\n## Body\nContent B",
				},
			}),
			request.post(getBackendUrl("/spaces/default/entries"), {
				data: {
					content: "---\nform: Entry\n---\n# Entry C\n\n## Body\nContent C",
				},
			}),
		]);

		const entries = (await Promise.all(
			formEntries.map((res) => res.json()),
		)) as Array<{ id: string }>;

		for (const entry of entries) {
			await page.goto(`/spaces/default/entries/${encodeURIComponent(entry.id)}`);
			await page.waitForLoadState("networkidle");
			const entryHtml = await page.content();
			expect(entryHtml).not.toContain("Loading entry...");
			expect(entryHtml).toContain("<div id=\"app\">");
		}

		for (const entry of entries) {
			await page.goto(`/spaces/default/entries/${encodeURIComponent(entry.id)}`);
			await page.waitForLoadState("networkidle");
			const html = await page.content();
			expect(html).not.toContain("Loading entry...");
			expect(html).toContain("<div id=\"app\">");
		}

		await Promise.all(
			entries.map((entry) =>
				request.delete(
					getBackendUrl(`/spaces/default/entries/${entry.id}`),
				),
			),
		);
	});

	test("REQ-FE-035: Navigation timeout handling and recovery", async ({ page, request }) => {
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					content:
						"---\nform: Entry\n---\n# Timeout Recovery Test\n\n## Body\nEnsure navigation resolves.",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		await page.goto(`/spaces/default/entries/${created.id}`);
		await page.waitForLoadState("networkidle");
		const html = await page.content();
		expect(html).not.toContain("Loading...");

		await request.delete(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
	});

	test("PUT /spaces/default/entries/:id updates entry", async ({ request }) => {
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					content:
						"---\nform: Entry\n---\n# Update Test Entry\n\n## Body\nOriginal content",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		const getRes = await request.get(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
		const current = (await getRes.json()) as { revision_id: string };

		const updateRes = await request.put(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
			{
				data: {
					markdown:
						"---\nform: Entry\n---\n# Updated Title\n\n## Body\nUpdated content by E2E test",
					parent_revision_id: current.revision_id,
				},
			},
		);
		expect(updateRes.ok()).toBeTruthy();

		await request.delete(getBackendUrl(`/spaces/default/entries/${created.id}`));
	});

	test("DELETE /spaces/default/entries/:id removes entry", async ({ request }) => {
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					content:
						"---\nform: Entry\n---\n# Delete Test Entry\n\n## Body\nTo be deleted",
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		const deleteRes = await request.delete(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
		expect([200, 204]).toContain(deleteRes.status());

		const fetchRes = await request.get(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
		expect(fetchRes.status()).toBe(404);
	});
});
