/**
 * Smoke Tests for IEapp
 *
 * These tests verify that the basic infrastructure is working:
 * - Frontend serves pages
 * - API endpoints respond correctly
 */

import { expect, test } from "@playwright/test";
import {
	ensureDefaultClass,
	getBackendUrl,
	getFrontendUrl,
	waitForServers,
} from "./lib/client";

test.describe("Smoke Tests", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
		await ensureDefaultClass(request);
	});

	test("GET / returns HTML with DOCTYPE", async ({ request }) => {
		const res = await request.get(getFrontendUrl("/"));
		expect(res.ok()).toBeTruthy();

		const body = await res.text();
		expect(body.toLowerCase()).toContain("<!doctype html>");
	});

	test("GET / has correct content-type", async ({ request }) => {
		const res = await request.get(getFrontendUrl("/"));
		const contentType = res.headers()["content-type"] ?? "";
		expect(contentType).toContain("text/html");
	});

	test("GET /workspaces returns HTML", async ({ request }) => {
		const res = await request.get(getFrontendUrl("/workspaces"));
		expect(res.ok()).toBeTruthy();

		const body = await res.text();
		expect(body.toLowerCase()).toContain("<!doctype html>");
	});

	test("GET /workspaces/default/notes/:id returns HTML", async ({ request }) => {
		const createRes = await request.post(
			getBackendUrl("/workspaces/default/notes"),
			{
				data: {
					content: `---\nclass: Note\n---\n# E2E Detail Route Note\n\n## Body\nCreated at ${new Date().toISOString()}`,
				},
			},
		);
		expect(createRes.status()).toBe(201);

		const created = (await createRes.json()) as { id: string };
		expect(created).toHaveProperty("id");

		const res = await request.get(
			getFrontendUrl(`/workspaces/default/notes/${created.id}`),
		);
		expect(res.ok()).toBeTruthy();

		const body = await res.text();
		expect(body.toLowerCase()).toContain("<!doctype html>");

		await request.delete(
			getBackendUrl(`/workspaces/default/notes/${created.id}`),
		);
	});

	test("GET /about returns HTML", async ({ request }) => {
		const res = await request.get(getFrontendUrl("/about"));
		expect(res.ok()).toBeTruthy();

		const body = await res.text();
		expect(body.toLowerCase()).toContain("<!doctype html>");
	});

	test("GET /workspaces returns list", async ({ request }) => {
		const res = await request.get(getBackendUrl("/workspaces"));
		expect(res.ok()).toBeTruthy();

		const json = await res.json();
		expect(Array.isArray(json)).toBe(true);
	});

	test("GET /workspaces includes default workspace", async ({ request }) => {
		const res = await request.get(getBackendUrl("/workspaces"));
		const workspaces = (await res.json()) as Array<{ name: string }>;

		const defaultWs = workspaces.find((ws) => ws.name === "default");
		expect(defaultWs).toBeDefined();
	});

	test("GET /workspaces/default/notes returns list", async ({ request }) => {
		const res = await request.get(getBackendUrl("/workspaces/default/notes"));
		expect(res.ok()).toBeTruthy();

		const json = await res.json();
		expect(Array.isArray(json)).toBe(true);
	});

	test("GET /nonexistent-api returns 404", async ({ request }) => {
		const res = await request.get(
			getBackendUrl("/nonexistent-endpoint-xyz"),
		);
		expect(res.status()).toBe(404);
	});
});
