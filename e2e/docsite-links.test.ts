import { expect, test } from "@playwright/test";
import { startDocsiteServer, type DocsiteServer } from "./support/docsite-server";

let docsiteServer: DocsiteServer | undefined;

test.describe("Docsite internal links", () => {
	test.beforeAll(async () => {
		test.setTimeout(180_000);
		docsiteServer = await startDocsiteServer({ basePath: "/ugoite" });
	});

	test.afterAll(async () => {
		await docsiteServer?.stop();
	});

	test("REQ-E2E-006: docsite internal page links resolve without 404s", async ({ page }) => {
		const queue = [buildDocsiteUrl("/")];
		const visited = new Set<string>();

		while (queue.length > 0) {
			const currentUrl = queue.shift();
			if (!currentUrl) {
				continue;
			}

			const normalizedCurrentUrl = normalizeCrawlUrl(currentUrl);
			if (visited.has(normalizedCurrentUrl)) {
				continue;
			}
			visited.add(normalizedCurrentUrl);

			const response = await page.goto(currentUrl, { waitUntil: "domcontentloaded" });
			expect(response, `Missing navigation response for ${currentUrl}`).not.toBeNull();
			expect(response!.status(), `Expected ${currentUrl} to resolve`).toBeLessThan(400);

			const hrefs = await page.locator("a[href]").evaluateAll((anchors) =>
				anchors.map((anchor) =>
					anchor instanceof HTMLAnchorElement ? anchor.href : "",
				),
			);

			for (const href of hrefs) {
				const normalizedHref = normalizeDocsiteHref(href);
				if (!normalizedHref) {
					continue;
				}
				if (!visited.has(normalizedHref)) {
					queue.push(normalizedHref);
				}
			}
		}

		expect(
			visited.size,
			"Expected to crawl a substantial set of docsite pages",
		).toBeGreaterThan(20);
	});
});

function buildDocsiteUrl(path: string): string {
	if (!docsiteServer) {
		throw new Error("Docsite server is unavailable");
	}
	return docsiteServer.buildUrl(path);
}

function normalizeCrawlUrl(rawUrl: string): string {
	const url = new URL(rawUrl);
	url.hash = "";
	url.search = "";
	if (url.pathname !== "/") {
		url.pathname = url.pathname.replace(/\/+$/, "");
	}
	return url.toString();
}

function normalizeDocsiteHref(rawHref: string): string | null {
	if (!rawHref || !docsiteServer) {
		return null;
	}

	const href = new URL(rawHref);
	const baseUrl = new URL(docsiteServer.getBaseUrl());
	if (href.origin !== baseUrl.origin) {
		return null;
	}

	const normalizedBasePath = baseUrl.pathname.endsWith("/")
		? baseUrl.pathname
		: `${baseUrl.pathname}/`;
	if (!href.pathname.startsWith(normalizedBasePath)) {
		return null;
	}

	if (/\.(json|png|jpe?g|svg|ico|css|js)$/i.test(href.pathname)) {
		return null;
	}

	return normalizeCrawlUrl(href.toString());
}
