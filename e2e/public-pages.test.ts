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

	test("REQ-OPS-015: home page Log in is the primary CTA and Open Spaces is secondary for unauthenticated visitors", async ({
		browser,
	}) => {
		const context = await browser.newContext({
			baseURL: process.env.FRONTEND_URL ?? "http://localhost:3000",
		});
		try {
			const page = await context.newPage();
			await page.goto("/");
			await page.waitForLoadState("networkidle");

			const loginLink = page.getByRole("link", { name: "Log in" });
			const spacesLink = page.getByRole("link", { name: "Open Spaces" });

			await expect(loginLink).toHaveAttribute("href", "/login");
			await expect(loginLink).toHaveClass(/ui-button-primary/);
			await expect(spacesLink).toHaveAttribute("href", "/spaces");
			await expect(spacesLink).toHaveClass(/ui-button-secondary/);
			await expect(page.locator("body")).toContainText(
				"/spaces requires an authenticated browser session",
			);
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

			await expect(
				page.getByRole("heading", { name: "Page not found" }),
			).toBeVisible();
			await expect(page.locator("body")).toContainText("still inside Ugoite");
			await expect(
				page.getByRole("link", { name: "Open Spaces" }),
			).toHaveAttribute("href", "/spaces");
			await expect(
				page.getByRole("link", { name: "Go to Login" }),
			).toHaveAttribute("href", "/login");
			await expect(
				page.getByRole("link", { name: "Back to Home" }),
			).toHaveAttribute("href", "/");
			await expect(
				page.getByRole("link", { name: "About Ugoite" }),
			).toHaveAttribute("href", "/about");
			await expect(page.locator("body")).not.toContainText("Visit solidjs.com");
			await expect(page.locator("body")).not.toContainText("About Page");
		} finally {
			await context.close();
		}
	});

test("REQ-FE-064: public landing pages respect the saved Japanese locale", async ({
		browser,
	}) => {
		const frontendUrl = process.env.FRONTEND_URL ?? "http://localhost:3000";
		const context = await browser.newContext({
			baseURL: frontendUrl,
			storageState: {
				cookies: [],
				origins: [
					{
						origin: frontendUrl,
						localStorage: [{ name: "ugoite-locale", value: "ja" }],
					},
				],
			},
		});
		try {
			const page = await context.newPage();
			await page.goto("/");
			await page.waitForLoadState("networkidle");

			await expect(page.locator("html")).toHaveAttribute("lang", "ja");
			await expect(page.locator("html")).toHaveAttribute("data-locale", "ja");
			await expect(page.locator("main")).toContainText(
				"ローカルファーストの知識を、検索と自動化のために構造化",
			);
			await expect(page.locator("main").getByRole("link", { name: "詳しく見る" })).toBeVisible();

			await page.goto("/about");
			await page.waitForLoadState("networkidle");

			await expect(page.locator("html")).toHaveAttribute("lang", "ja");
			await expect(page.locator("html")).toHaveAttribute("data-locale", "ja");
			await expect(
				page.locator("main").getByRole("heading", { name: "Ugoite について" }),
			).toBeVisible();
			await expect(page.locator("main")).toContainText("柔軟な構造と高速な検索");
			await expect(
				page.locator("main").getByRole("link", { name: "ホームに戻る" }),
			).toHaveAttribute("href", "/");
		} finally {
			await context.close();
		}
	});
});
