import { expect, test } from "@playwright/test";
import {
  type DocsiteServer,
  startDocsiteServer,
} from "./support/docsite-server.ts";

const homepagePath = "/";

let docsiteServer: DocsiteServer | undefined;

test.describe("Docsite navigation smoke", () => {
  test.beforeAll(async () => {
    test.setTimeout(180_000);
    docsiteServer = await startDocsiteServer();
  });

  test.afterAll(async () => {
    await docsiteServer?.stop();
  });

  test("REQ-E2E-005: homepage exposes canonical documentation paths", async ({ page }) => {
    // Mitase evidence: REQ-E2E-005#criterion.mobile-sidebar.
    // Mitase evidence: REQ-E2E-005#criterion.desktop-sidebar.
    // Mitase evidence: REQ-E2E-005#criterion.beginner-path.
    // Mitase evidence: REQ-E2E-005#criterion.homepage-navigation.
    // Mitase evidence: REQ-E2E-009#criterion.desktop-layout.
    // Mitase evidence: REQ-E2E-009#criterion.responsive-layout.
    for (
      const viewport of [
        { width: 390, height: 844 },
        { width: 1440, height: 900 },
      ]
    ) {
      await page.setViewportSize(viewport);
      await page.goto(buildDocsiteUrl(homepagePath), {
        waitUntil: "networkidle",
      });

      await expect(
        page.getByRole("heading", { level: 1, name: "Ugoite" }),
      ).toBeVisible();
      for (const [label, route] of canonicalPaths) {
        const link = page.getByRole("link", { name: label, exact: true })
          .first();
        await expect(link).toBeVisible();
        await expect(link).toHaveAttribute(
          "href",
          new RegExp(`${route.slice(1)}/?$`),
        );
      }
    }

    await page.getByRole("link", { name: "Get started", exact: true })
      .first()
      .click();
    await expect(page).toHaveURL(/\/docs\/get-started\/?$/);

    const quickstart = page.getByRole("link", {
      name: "Quickstart",
      exact: true,
    }).first();
    await expect(quickstart).toBeVisible();
    await quickstart.click();
    await expect(page).toHaveURL(/\/docs\/get-started\/quickstart\/?$/);

    await page.goto(buildDocsiteUrl(homepagePath), {
      waitUntil: "networkidle",
    });
    await page.getByRole("link", { name: "Architecture & Specification", exact: true })
      .first()
      .click();
    await expect(page).toHaveURL(/\/docs\/spec\/?$/);
  });
});

const canonicalPaths = [
  ["Get started", "/docs/get-started"],
  ["Knowledge tasks", "/docs/use"],
  ["Operations", "/docs/operate"],
  ["Development", "/docs/develop"],
  ["Reference", "/docs/reference"],
  ["Architecture & Specification", "/docs/spec"],
] as const;

function buildDocsiteUrl(path: string): string {
  if (!docsiteServer) {
    throw new Error("Docsite server is unavailable");
  }
  return docsiteServer.buildUrl(path);
}
