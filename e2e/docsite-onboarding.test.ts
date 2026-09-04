import { expect, type Page, test } from "@playwright/test";
import {
  type DocsiteServer,
  startDocsiteServer,
} from "./support/docsite-server.ts";

let docsiteServer: DocsiteServer | undefined;

test.describe("Docsite onboarding", () => {
  test.beforeAll(async () => {
    test.setTimeout(180_000);
    docsiteServer = await startDocsiteServer();
  });

  test.afterAll(async () => {
    await docsiteServer?.stop();
  });

  test("REQ-E2E-008: the canonical docs landing page gives users concrete start paths", async ({ page }) => {
    // Mitase evidence: REQ-E2E-008#criterion.landing-content.
    // Mitase evidence: REQ-E2E-008#criterion.start-paths.
    await page.goto(buildDocsiteUrl("/"), { waitUntil: "networkidle" });

    await expect(page.getByRole("heading", { level: 1, name: "Ugoite" }))
      .toBeVisible();
    await expect(
      page.getByText(/private, portable Knowledge Space for humans and AI/i),
    ).toBeVisible();
    await expect(primaryQuickStartCta(page)).toBeVisible();
    await expect(page.getByText(/operator-owned Space/i)).toBeVisible();
    await expect(page.getByRole("link", { name: "Run from source" }))
      .toBeVisible();
    await expect(page.getByRole("link", { name: "View on GitHub" }))
      .toBeVisible();
    await expect(page.getByText("Browser caveat today")).toBeVisible();
    await expect(page.getByRole("heading", { level: 2, name: "Choose a path" }))
      .toBeVisible();
    await expect(
      page.getByRole("heading", { level: 2, name: "Source-of-truth rules" }),
    ).toBeVisible();
  });

  test("REQ-E2E-008: the primary action opens the canonical quick-start document", async ({ page }) => {
    // Mitase evidence: REQ-E2E-008#criterion.start-paths.
    await page.goto(buildDocsiteUrl("/"), { waitUntil: "networkidle" });
    await primaryQuickStartCta(page).click();

    await expect(page).toHaveURL(
      /\/docs\/guide\/start\/container-quickstart\/$/,
    );
    await expect(
      page.getByRole("heading", { level: 1, name: /container quick start/i }),
    ).toBeVisible();
  });
});

function buildDocsiteUrl(path: string): string {
  if (!docsiteServer) {
    throw new Error("Docsite server is unavailable");
  }
  return docsiteServer.buildUrl(path);
}

function primaryQuickStartCta(page: Page) {
  return page.getByRole("main").getByRole("link", {
    name: "Container quick start",
    exact: true,
  });
}
