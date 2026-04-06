/**
 * Smoke Tests for Ugoite
 *
 * These tests verify that the basic infrastructure is working:
 * - Frontend serves pages
 * - API endpoints respond correctly
 */

import { expect, test } from "@playwright/test";
import { ensureDefaultForm, getBackendUrl, waitForServers } from "./lib/client";

test.describe("Smoke Tests", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
		await ensureDefaultForm(request);
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

	test("GET /spaces returns HTML", async ({ page }) => {
		await page.goto("/spaces");
		await page.waitForLoadState("networkidle");
		const body = await page.content();
		expect(body.toLowerCase()).toContain("<!doctype html>");
	});

	test("GET /spaces/default/entries/:id returns HTML", async ({ page, request }) => {
		const createRes = await request.post(
			getBackendUrl("/spaces/default/entries"),
			{
				data: {
					content: `---\nform: Entry\n---\n# E2E Detail Route Entry\n\n## Body\nCreated at ${new Date().toISOString()}`,
				},
			},
		);
		expect(createRes.status()).toBe(201);
		const created = (await createRes.json()) as { id: string };

		await page.goto(`/spaces/default/entries/${created.id}`);
		await page.waitForLoadState("networkidle");
		const body = await page.content();
		expect(body.toLowerCase()).toContain("<!doctype html>");

		await request.delete(
			getBackendUrl(`/spaces/default/entries/${created.id}`),
		);
	});

	test("GET /about returns HTML", async ({ page }) => {
		await page.goto("/about");
		await page.waitForLoadState("networkidle");
		const body = await page.content();
		expect(body.toLowerCase()).toContain("<!doctype html>");
	});

	test("REQ-OPS-015: browser mock-oauth login reaches the spaces page", async ({ browser }) => {
		const context = await browser.newContext();
		const page = await context.newPage();

		try {
			await page.goto("/login");
			await page.getByRole("button", { name: "Continue with Local Demo Login" }).click();
			await expect(page).toHaveURL(/\/spaces$/);
			await expect(page.getByText("Available Spaces")).toBeVisible();
		} finally {
			await context.close();
		}
	});

	test("REQ-OPS-015: visiting login does not clear an existing browser auth cookie", async ({
		browser,
	}) => {
		const frontendUrl = process.env.FRONTEND_URL ?? "http://localhost:3000";
		const frontendOrigin = new URL(frontendUrl);
		const context = await browser.newContext();
		await context.addCookies([
			{
				name: "ugoite_auth_bearer_token",
				value: "existing-browser-session",
				domain: frontendOrigin.hostname,
				path: "/",
				httpOnly: false,
				secure: frontendOrigin.protocol === "https:",
				sameSite: "Lax",
				expires: Math.floor(Date.now() / 1000) + 3_600,
			},
		]);
		const page = await context.newPage();

		try {
			await page.goto("/login");
			await page.waitForLoadState("networkidle");
			await expect(page.getByRole("button", { name: "Continue with Local Demo Login" })).toBeVisible();
			const cookies = await context.cookies(frontendUrl);
			expect(
				cookies.find((cookie) => cookie.name === "ugoite_auth_bearer_token")?.value,
			).toBe("existing-browser-session");
		} finally {
			await context.close();
		}
	});

	test("GET /spaces returns list", async ({ request }) => {
		const res = await request.get(getBackendUrl("/spaces"));
		expect(res.ok()).toBeTruthy();

		const json = await res.json();
		expect(Array.isArray(json)).toBe(true);
	});

	test("GET /spaces includes default space", async ({ request }) => {
		const res = await request.get(getBackendUrl("/spaces"));
		const spaces = (await res.json()) as Array<{ name: string }>;
		const defaultWs = spaces.find((ws) => ws.name === "default");
		expect(defaultWs).toBeDefined();
	});

	test("GET /spaces/default/entries returns list", async ({ request }) => {
		const res = await request.get(getBackendUrl("/spaces/default/entries"));
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
