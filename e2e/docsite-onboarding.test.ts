import { expect, test } from "@playwright/test";
import { startDocsiteServer, type DocsiteServer } from "./support/docsite-server";

let docsiteServer: DocsiteServer | undefined;

test.describe("Docsite onboarding-first navigation", () => {
	test.beforeAll(async () => {
		test.setTimeout(180_000);
		docsiteServer = await startDocsiteServer();
	});

	test.afterAll(async () => {
		await docsiteServer?.stop();
	});

	test("REQ-E2E-008: landing page explains what Ugoite is and points first-time users to getting started", async ({
		page,
	}) => {
		await page.goto(buildDocsiteUrl("/"), { waitUntil: "networkidle" });

		await expect(
			page.getByRole("heading", { level: 1, name: "Ugoite" }),
		).toBeVisible();
		await expect(
			page.getByText(
				/a private, portable knowledge space you can run with docker/i,
			),
		).toBeVisible();
		await expect(page.locator("#start-paths a h3")).toHaveText([
			"Try the published release",
			"Run from source",
			"Use the CLI",
		]);
		await expect(page.locator("#next-steps a h3")).toHaveText([
			"Explore the browser app",
			"Understand auth and access",
			"Read design and source docs",
		]);

		const getStartedLink = page.getByRole("link", { name: "Get Started" });
		await expect(getStartedLink).toBeVisible();
		await getStartedLink.click();
		await expect(page).toHaveURL(/\/getting-started$/);
		await expect(
			page.getByRole("heading", {
				level: 1,
				name: /start with ugoite before diving into design docs/i,
			}),
		).toBeVisible();
		await expect(page.locator("#first-steps .doc-card h2")).toHaveText([
			"Try the published release",
			"Run from source",
			"Use the CLI",
		]);
		await expect(page.locator("#next .doc-card h3")).toHaveText([
			"Explore the browser app",
			"Understand auth and access",
			"Read design and source docs",
		]);
	});

	test("REQ-E2E-008: desktop navigation prioritizes getting-started content before design docs", async ({
		page,
	}) => {
		await page.setViewportSize({ width: 1280, height: 900 });
		await page.goto(buildDocsiteUrl("/getting-started"), {
			waitUntil: "networkidle",
		});

		await expect(page.locator(".site-nav-menu > a")).toHaveText([
			"Home",
			"Getting Started",
			"Application",
			"Design",
			"Source Docs",
		]);
		await expect(
			page.locator(".site-nav-menu").nth(1).locator(".site-nav-submenu a"),
		).toHaveText([
			"Core Concepts",
			"Container Quickstart",
			"Docker Compose",
			"Auth Overview",
			"CLI Guide",
		]);
	});

	test("REQ-E2E-008: mobile navigation keeps getting-started links ahead of design content", async ({
		page,
	}) => {
		await page.setViewportSize({ width: 390, height: 844 });
		await page.goto(buildDocsiteUrl("/getting-started"), {
			waitUntil: "networkidle",
		});

		await page.locator(".mobile-nav-toggle").click();
		await expect(
			page.locator(".mobile-top-links .doc-sidebar-link"),
		).toHaveText([
			"Home",
			"Getting Started",
			"Application",
			"Design",
			"Source Docs",
			"GitHub",
		]);
		await expect(
			page.locator("#mobile-doc-nav .doc-sidebar-title"),
		).toHaveText([
			"Getting Started",
			"Application",
			"Design Principles",
			"Source Docs",
		]);
	});
});

function buildDocsiteUrl(path: string): string {
	if (!docsiteServer) {
		throw new Error("Docsite server is unavailable");
	}
	return docsiteServer.buildUrl(path);
}
