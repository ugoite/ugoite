import { expect, test } from "@playwright/test";
import {
  type DocsiteServer,
  startDocsiteServer,
} from "./support/docsite-server.ts";

const docPath = "/docs/spec/";

let docsiteServer: DocsiteServer | undefined;

test.describe("Docsite navigation layout", () => {
  test.beforeAll(async () => {
    test.setTimeout(180_000);
    docsiteServer = await startDocsiteServer();
  });

  test.afterAll(async () => {
    await docsiteServer?.stop();
  });

  test("REQ-E2E-005: Starlight exposes the documentation sidebar as a mobile menu", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

    const menuButton = page.getByRole("button", { name: "Menu" });
    const menuHost = page.locator("starlight-menu-button");
    const sidebar = page.locator("#starlight__sidebar");

    await expect(menuButton).toBeVisible();
    await expect(menuHost).toHaveAttribute("aria-expanded", "false");
    await expect(sidebar).toBeHidden();

    await menuButton.click();
    await expect(menuHost).toHaveAttribute("aria-expanded", "true");
    await expect(sidebar).toBeVisible();
    await expect(page.locator("body")).toHaveAttribute(
      "data-mobile-menu-expanded",
      "",
    );
    await expect(
      page.locator('#starlight__sidebar nav[aria-label="Main"]'),
    ).toBeVisible();
  });

  test("REQ-E2E-005: the mobile menu closes with Escape", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

    const menuButton = page.getByRole("button", { name: "Menu" });
    const menuHost = page.locator("starlight-menu-button");
    await menuButton.click();
    await expect(menuHost).toHaveAttribute("aria-expanded", "true");

    await menuButton.press("Escape");
    await expect(menuHost).toHaveAttribute("aria-expanded", "false");
    await expect(page.locator("body")).not.toHaveAttribute(
      "data-mobile-menu-expanded",
      /.+/,
    );
  });

  test("REQ-E2E-009: desktop pages use Starlight's sidebar and table of contents", async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

    await expect(page.getByRole("button", { name: "Menu" })).toBeHidden();
    await expect(page.locator("#starlight__sidebar")).toBeVisible();
    await expect(page.locator('nav.sidebar[aria-label="Main"]')).toBeVisible();
    await expect(page.locator(".right-sidebar-container")).toBeVisible();
  });
});

function buildDocsiteUrl(path: string): string {
  if (!docsiteServer) {
    throw new Error("Docsite server is unavailable");
  }
  return docsiteServer.buildUrl(path);
}
