import { expect, test } from "@playwright/test";
import { waitForServers } from "./lib/client";

test.describe("Public page stability", () => {
	test.beforeAll(async ({ request }) => {
		await waitForServers(request);
	});

	test("REQ-E2E-004: public pages avoid auth-only portable preference requests", async ({
		browser,
	}) => {
		const context = await browser.newContext({
			baseURL: process.env.FRONTEND_URL ?? "http://localhost:3000",
		});
		try {
			const page = await context.newPage();
			const preferenceRequests: string[] = [];
			const consoleErrors: string[] = [];

			page.on("request", (request) => {
				if (request.url().includes("/api/preferences/me")) {
					preferenceRequests.push(`${request.method()} ${request.url()}`);
				}
			});
			page.on("console", (message) => {
				if (message.type() === "error") {
					consoleErrors.push(message.text());
				}
			});

			for (const pathname of ["/", "/about", "/does-not-exist"]) {
				await page.goto(pathname);
				await page.waitForLoadState("networkidle");
			}

			expect(preferenceRequests).toEqual([]);
			expect(consoleErrors).toEqual([]);
		} finally {
			await context.close();
		}
	});

	test("REQ-E2E-004: unknown routes show Ugoite recovery guidance instead of scaffold copy", async ({
		browser,
	}) => {
		const context = await browser.newContext({
			baseURL: process.env.FRONTEND_URL ?? "http://localhost:3000",
		});
		try {
			const page = await context.newPage();
			await page.goto("/does-not-exist");
			await page.waitForLoadState("networkidle");

			await expect(page.getByRole("heading", { name: "Page not found" })).toBeVisible();
			await expect(page.locator("body")).toContainText("still inside Ugoite");
			await expect(page.getByRole("link", { name: "Open Spaces" })).toHaveAttribute(
				"href",
				"/spaces",
			);
			await expect(page.getByRole("link", { name: "Go to Login" })).toHaveAttribute(
				"href",
				"/login",
			);
			await expect(page.getByRole("link", { name: "Back to Home" })).toHaveAttribute(
				"href",
				"/",
			);
			await expect(page.getByRole("link", { name: "About Ugoite" })).toHaveAttribute(
				"href",
				"/about",
			);
			await expect(page.locator("body")).not.toContainText("Visit solidjs.com");
			await expect(page.locator("body")).not.toContainText("About Page");
		} finally {
			await context.close();
		}
	});
});
