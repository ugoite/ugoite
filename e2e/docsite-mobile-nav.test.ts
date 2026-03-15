import { expect, test } from "@playwright/test";
import { startDocsiteServer, type DocsiteServer } from "./support/docsite-server";

const docPath = "/docs/spec/index";

let docsiteServer: DocsiteServer | undefined;

test.describe("Docsite mobile navigation", () => {
	test.beforeAll(async () => {
		test.setTimeout(180_000);
		docsiteServer = await startDocsiteServer();
	});

	test.afterAll(async () => {
		await docsiteServer?.stop();
	});

	test("REQ-E2E-005: mobile nav opens as drawer and keeps reading area uncluttered", async ({
		page,
	}) => {
		await page.setViewportSize({ width: 390, height: 844 });
		await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

		await expect(page.locator(".mobile-nav-toggle")).toBeVisible();
		await expect(page.locator(".site-nav")).toBeHidden();
		await expect(page.locator("main .doc-sidebar").first()).toBeHidden();

		await page.locator(".mobile-nav-toggle").click();
		await expect(page.locator(".mobile-doc-nav")).toHaveClass(/is-open/);
		await expect(page.locator(".mobile-nav-overlay")).toBeVisible();
		await expect(page.locator(".mobile-doc-nav .doc-sidebar-link").first()).toBeVisible();
		await expect(page.locator("body")).toHaveClass(/mobile-nav-open/);
	});

	test("REQ-E2E-005: mobile nav closes by escape and link tap", async ({ page }) => {
		await page.setViewportSize({ width: 390, height: 844 });
		await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

		await page.locator(".mobile-nav-toggle").click();
		await expect(page.locator(".mobile-doc-nav")).toHaveClass(/is-open/);
		await page.keyboard.press("Escape");
		await expect(page.locator(".mobile-doc-nav")).not.toHaveClass(/is-open/);

		await page.locator(".mobile-nav-toggle").click();
		await expect(page.locator(".mobile-doc-nav")).toHaveClass(/is-open/);
		await page.locator(".mobile-doc-nav .doc-sidebar-link").first().click();
		await expect(page.locator(".mobile-doc-nav")).not.toHaveClass(/is-open/);
	});

	test("REQ-E2E-005: mobile nav closes by overlay click", async ({ page }) => {
		await page.setViewportSize({ width: 390, height: 844 });
		await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

		await page.locator(".mobile-nav-toggle").click();
		await expect(page.locator(".mobile-doc-nav")).toHaveClass(/is-open/);
		const overlay = page.locator(".mobile-nav-overlay");
		await expect(overlay).toBeVisible();

		const overlayBox = await overlay.boundingBox();
		if (!overlayBox) {
			throw new Error("Overlay bounding box is unavailable");
		}
		await page.mouse.click(
			overlayBox.x + overlayBox.width - 6,
			overlayBox.y + Math.min(40, overlayBox.height - 6),
		);
		await expect(page.locator(".mobile-doc-nav")).not.toHaveClass(/is-open/);
	});
});

function buildDocsiteUrl(path: string): string {
	if (!docsiteServer) {
		throw new Error("Docsite server is unavailable");
	}
	return docsiteServer.buildUrl(path);
}
