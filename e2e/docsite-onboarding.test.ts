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
		await expect(page.getByText("Browser caveat today")).toBeVisible();
		await expect(
			page.getByText(
				/the browser path is still server-backed and login-gated, even though the data stays local-first/i,
			),
		).toBeVisible();
		await expect(
			page.getByText(
				/the current browser route still needs a running backend \+ frontend stack and an explicit login flow/i,
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

	test("REQ-E2E-008: landing page labels the application overview CTA as documentation", async ({
		page,
	}) => {
		await page.goto(buildDocsiteUrl("/"), { waitUntil: "networkidle" });

		const applicationDocsLink = page.getByRole("link", {
			name: "Explore Application Docs",
		});
		await expect(applicationDocsLink).toBeVisible();
		await expect(applicationDocsLink).toHaveAttribute("href", /\/app$/);
		await expect(
			page.getByRole("link", { name: "Browse Application" }),
		).toHaveCount(0);

		await applicationDocsLink.click();

		await expect(page).toHaveURL(/\/app$/);
		await expect(
			page.getByRole("heading", {
				level: 1,
				name: /run your knowledge space from anywhere/i,
			}),
		).toBeVisible();
		await expect(page.getByText("Application Docs")).toBeVisible();
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
		await expect(
			page.locator(".site-nav-menu").nth(2).locator(".site-nav-submenu a"),
		).toHaveCount(0);
		await expect(
			page.locator(".site-nav-menu").nth(3).locator(".site-nav-submenu a"),
		).toHaveCount(0);
		await expect(
			page.locator(".site-nav-menu").nth(4).locator(".site-nav-submenu a"),
		).toHaveCount(0);
	});

	test("REQ-E2E-008: run from source card opens the canonical host-dev workflow", async ({
		page,
	}) => {
		await page.goto(buildDocsiteUrl("/getting-started"), {
			waitUntil: "networkidle",
		});

		const card = page.locator("#first-steps a", { hasText: "Run from source" });
		await expect(card).toHaveAttribute(
			"href",
			/\/docs\/guide\/local-dev-auth-login$/,
		);

		await card.click();

		await expect(page).toHaveURL(/\/docs\/guide\/local-dev-auth-login$/);
		await expect(
			page.getByRole("heading", {
				level: 1,
				name: /local development authentication and login/i,
			}),
		).toBeVisible();
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
		).toHaveText(["Getting Started"]);
	});
});

function buildDocsiteUrl(path: string): string {
	if (!docsiteServer) {
		throw new Error("Docsite server is unavailable");
	}
	return docsiteServer.buildUrl(path);
}
