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
			const failingPreferenceRequests: string[] = [];
			const consoleErrors: string[] = [];

			page.on("response", (response) => {
				if (
					response.url().includes("/api/preferences/me") &&
					response.status() >= 400
				) {
					failingPreferenceRequests.push(`${response.status()} ${response.url()}`);
				}
			});
			page.on("console", (message) => {
				if (message.type() === "error") {
					consoleErrors.push(message.text());
				}
			});

			for (const pathname of ["/", "/about"]) {
				await page.goto(pathname);
				await page.waitForLoadState("networkidle");
			}

			expect(failingPreferenceRequests).toEqual([]);
			expect(consoleErrors).toEqual([]);
		} finally {
			await context.close();
		}
	});
});
