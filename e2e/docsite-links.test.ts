import { expect, type Page, test } from "@playwright/test";
import {
  type DocsiteServer,
  startDocsiteServer,
} from "./support/docsite-server.ts";

let docsiteServer: DocsiteServer | undefined;

test.describe("Docsite internal links", () => {
  test.describe.configure({ timeout: 180_000 });

  test.beforeAll(async () => {
    docsiteServer = await startDocsiteServer({ basePath: "/ugoite" });
  });

  test.afterAll(async () => {
    await docsiteServer?.stop();
  });

  test("REQ-E2E-006: docsite internal page links resolve without 404s", async ({ browser, request }) => {
    // Mitase evidence: REQ-E2E-006#criterion.base-path-integrity.
    test.setTimeout(180_000);
    const queue = [buildDocsiteUrl("/")];
    const visited = new Set<string>();
    const parserPage = await browser.newPage();

    try {
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
        expect(response.status(), `Expected ${currentUrl} to resolve`)
          .toBeLessThan(400);

        const resolvedUrl = normalizeCrawlUrl(response.url());
        visited.add(normalizedCurrentUrl);
        visited.add(resolvedUrl);

        const contentType = response.headers()["content-type"] ?? "";
        if (!contentType.includes("text/html")) {
          continue;
        }

        const hrefs = await extractPageHrefs(parserPage, await response.text());

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
    } finally {
      await parserPage.close();
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

  // Keep extensionless documentation routes as directory URLs. Removing the
  // trailing slash changes how relative Markdown links resolve in dev mode
  // (for example, `architecture/contracts/overview/` from
  // `/docs/architecture/quality/`).
  const finalSegment = url.pathname.split("/").at(-1) ?? "";
  if (
    url.pathname !== "/" &&
    !url.pathname.endsWith("/") &&
    !finalSegment.includes(".")
  ) {
    url.pathname = `${url.pathname}/`;
  }

  return url.toString();
}

function normalizeDocsiteHref(
  rawHref: string,
  currentUrl: string,
): string | null {
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

  if (
    /\.(json|png|jpe?g|webp|svg|ico|css|js|map|xml|txt|woff2?)$/i.test(
      href.pathname,
    )
  ) {
    return null;
  }

  return normalizeCrawlUrl(href.toString());
}

async function extractPageHrefs(page: Page, html: string): Promise<string[]> {
  await page.setContent(html, { waitUntil: "domcontentloaded" });
  return page.locator("a[href]").evaluateAll((anchors) =>
    anchors.map((anchor) =>
      anchor instanceof HTMLAnchorElement
        ? anchor.getAttribute("href") ?? ""
        : ""
    )
  );
}
