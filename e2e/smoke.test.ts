/**
 * Smoke Tests for IEapp
 *
 * These tests verify that the basic infrastructure is working:
 * - Frontend serves pages
 * - API endpoints respond correctly
 */

import { expect, test } from "@playwright/test";
import {
	enableBackendProxy,
	ensureDefaultClass,
	getBackendUrl,
	waitForServers,
} from "./lib/client";

test.describe("Smoke Tests", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
		await ensureDefaultClass(request);
	});

	test.beforeEach(async ({ page }) => {
		await enableBackendProxy(page);
	});

	test("GET / returns HTML with DOCTYPE", async ({ page }) => {
		await page.goto("/");
		await page.waitForLoadState("networkidle");
		const body = await page.content();
		expect(body.toLowerCase()).toContain("<!doctype html>");
	});

	test("GET / has correct content-type", async ({ page }) => {
		const response = await page.goto("/");
		expect(response).not.toBeNull();
		const contentType = response?.headers()["content-type"] ?? "";
		expect(contentType).toContain("text/html");
	});

	test("GET /workspaces returns HTML", async ({ page }) => {
		await page.goto("/workspaces");
		await page.waitForLoadState("networkidle");
		const body = await page.content();
		expect(body.toLowerCase()).toContain("<!doctype html>");
	});

	test("GET /workspaces/default/notes/:id returns HTML", async ({ page, request }) => {
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

		await page.goto(`/workspaces/default/notes/${created.id}`);
		await page.waitForLoadState("networkidle");
		const body = await page.content();
		expect(body.toLowerCase()).toContain("<!doctype html>");

		await request.delete(
			getBackendUrl(`/workspaces/default/notes/${created.id}`),
		);
	});

	test("GET /about returns HTML", async ({ page }) => {
		await page.goto("/about");
		await page.waitForLoadState("networkidle");
		const body = await page.content();
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
