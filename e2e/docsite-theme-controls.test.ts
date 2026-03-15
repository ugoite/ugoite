import { expect, test } from "@playwright/test";
import { startDocsiteServer, type DocsiteServer } from "./support/docsite-server";

const docPath = "/docs/spec/index";
const fixedTheme = "materialize";

let docsiteServer: DocsiteServer | undefined;

test.describe("Docsite theme controls", () => {
	test.beforeAll(async () => {
		test.setTimeout(180_000);
		docsiteServer = await startDocsiteServer();
	});

	test.afterAll(async () => {
		await docsiteServer?.stop();
	});

	test("REQ-E2E-007: docsite keeps a fixed UI theme and only exposes color mode selection", async ({
		page,
	}) => {
		await page.addInitScript(() => {
			localStorage.setItem("ugoite-ui-theme", "classic");
			localStorage.setItem("ugoite-color-mode", "dark");
		});

		await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

		await expect(page.locator("[data-theme-selector]")).toHaveCount(0);
		await expect(page.locator("[data-mode-selector]")).toBeVisible();
		await expect(page.locator("html")).toHaveAttribute("data-ui-theme", fixedTheme);
		await expect(page.locator("html")).toHaveAttribute("data-color-mode", "dark");

		await page.locator("[data-mode-selector]").selectOption("light");
		await expect(page.locator("html")).toHaveAttribute("data-ui-theme", fixedTheme);
		await expect(page.locator("html")).toHaveAttribute("data-color-mode", "light");

		const storage = await page.evaluate(() => ({
			mode: localStorage.getItem("ugoite-color-mode"),
			theme: localStorage.getItem("ugoite-ui-theme"),
		}));
		expect(storage).toEqual({
			mode: "light",
			theme: "classic",
		});
	});
});

function buildDocsiteUrl(path: string): string {
	if (!docsiteServer) {
		throw new Error("Docsite server is unavailable");
	}
	return docsiteServer.buildUrl(path);
}
