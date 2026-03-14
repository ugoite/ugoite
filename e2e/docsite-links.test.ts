import { expect, test, type Page } from "@playwright/test";
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

	test("REQ-E2E-006: docsite internal page links resolve without 404s", async ({ page, request }) => {
		test.setTimeout(180_000);
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
			const response = await request.get(currentUrl);
			expect(response.status(), `Expected ${currentUrl} to resolve`).toBeLessThan(400);

			const resolvedUrl = normalizeCrawlUrl(response.url());
			visited.add(normalizedCurrentUrl);
			visited.add(resolvedUrl);

			const contentType = response.headers()["content-type"] ?? "";
			if (!contentType.includes("text/html")) {
				continue;
			}

			const hrefs = await extractPageHrefs(page, await response.text());

			for (const href of hrefs) {
				const normalizedHref = normalizeDocsiteHref(href, response.url());
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

function normalizeDocsiteHref(rawHref: string, currentUrl: string): string | null {
	if (!rawHref || !docsiteServer) {
		return null;
	}

	const href = new URL(rawHref, currentUrl);
	const baseUrl = new URL(docsiteServer.getBaseUrl());
	if (href.origin !== baseUrl.origin) {
		return null;
	}

	const normalizedBasePath = baseUrl.pathname.endsWith("/")
		? baseUrl.pathname
		: `${baseUrl.pathname}/`;
	if (!href.pathname.startsWith(normalizedBasePath)) {
		throw new Error(
			`Found same-origin link outside configured base path: ${href.pathname}. ` +
				`Expected all internal docsite links to start with "${normalizedBasePath}".`,
		);
	}

	if (/\.(json|png|jpe?g|svg|ico|css|js)$/i.test(href.pathname)) {
		return null;
	}

	return normalizeCrawlUrl(href.toString());
}

async function extractPageHrefs(page: Page, html: string): Promise<string[]> {
	return page.evaluate((markup) => {
		const document = new DOMParser().parseFromString(markup, "text/html");
		return Array.from(document.querySelectorAll("a[href]"), (anchor) =>
			anchor instanceof HTMLAnchorElement ? anchor.getAttribute("href") ?? "" : "",
		);
	}, html);
}
