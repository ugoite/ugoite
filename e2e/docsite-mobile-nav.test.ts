import { expect, test } from "@playwright/test";
import {
  type DocsiteServer,
  startDocsiteServer,
} from "./support/docsite-server.ts";

const docPath = "/docs/spec/";
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

  test("REQ-E2E-005: Starlight exposes the documentation sidebar as a mobile menu", async ({ page }) => {
    // Mitase evidence: REQ-E2E-005#criterion.mobile-sidebar.
    // Mitase evidence: REQ-E2E-009#criterion.responsive-layout.
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

    const menuButton = page.getByRole("button", { name: "Menu" });
    const sidebar = page.locator("#starlight__sidebar");

    await expect(menuButton).toBeVisible();
    await expect(sidebar).toBeHidden();

    await menuButton.click();
    await expect(sidebar).toBeVisible();
  });

  test("REQ-E2E-005: the mobile menu closes with Escape", async ({ page }) => {
    // Mitase evidence: REQ-E2E-005#criterion.mobile-sidebar.
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

    const menuButton = page.getByRole("button", { name: "Menu" });
    const sidebar = page.locator("#starlight__sidebar");
    await menuButton.click();
    await expect(sidebar).toBeVisible();

    await menuButton.press("Escape");
    await expect(sidebar).toBeHidden();
  });

  test("REQ-E2E-009: desktop pages use Starlight's sidebar and table of contents", async ({ page }) => {
    // Mitase evidence: REQ-E2E-005#criterion.desktop-sidebar.
    // Mitase evidence: REQ-E2E-009#criterion.desktop-layout.
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

    await expect(page.getByRole("button", { name: "Menu" })).toBeHidden();
    await expect(page.locator("#starlight__sidebar")).toBeVisible();
    // Minimum layout smoke for REQ-E2E-009#criterion.desktop-layout: the
    // framework-owned table of contents slot renders on a headed page.
    // Starlight owns its internals; no TOC item assertions live here.
    await expect(page.locator(".right-sidebar-container")).toBeVisible();
    await expect(
      page.getByRole("heading", { level: 1 }),
    ).toBeVisible();
  });

  test("REQ-E2E-005: the beginner path follows its documented learning order", async ({ page }) => {
    // Mitase evidence: REQ-E2E-005#criterion.beginner-path.
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto(buildDocsiteUrl("/docs/get-started/"), {
      waitUntil: "networkidle",
    });

    const getStartedLinks = page.locator(
      '#starlight__sidebar a[href*="/docs/get-started/"]',
    );
    await expect(getStartedLinks).toHaveText(["Overview", "Quickstart"]);
  });

  test("REQ-E2E-005: the homepage keeps the hero and Starlight navigation", async ({ page }) => {
    // Mitase evidence: REQ-E2E-005#criterion.homepage-navigation.
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(buildDocsiteUrl(homepagePath), {
      waitUntil: "networkidle",
    });

    await expect(page.getByText("A private, portable knowledge space"))
      .toBeVisible();

    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto(buildDocsiteUrl(homepagePath), {
      waitUntil: "networkidle",
    });
    await expect(page.locator("#starlight__sidebar")).toBeVisible();
  });
});

function buildDocsiteUrl(path: string): string {
  if (!docsiteServer) {
    throw new Error("Docsite server is unavailable");
  }
  return docsiteServer.buildUrl(path);
}
