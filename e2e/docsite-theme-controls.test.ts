import { expect, test } from "@playwright/test";
import {
  type DocsiteServer,
  startDocsiteServer,
} from "./support/docsite-server.ts";

const docPath = "/docs/spec/";

let docsiteServer: DocsiteServer | undefined;

test.describe("Docsite theme controls", () => {
  test.beforeAll(async () => {
    test.setTimeout(180_000);
    docsiteServer = await startDocsiteServer();
  });

  test.afterAll(async () => {
    await docsiteServer?.stop();
  });

  test("REQ-E2E-007: Starlight owns the single light/dark/auto selector", async ({ page }) => {
    // Mitase evidence: REQ-E2E-007#criterion.single-framework-selector.
    // Mitase evidence: REQ-E2E-007#criterion.no-product-selector.
    await page.addInitScript(() => {
      localStorage.setItem("starlight-theme", "dark");
    });
    await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

    const selector = page.locator("starlight-theme-select select");
    await expect(selector).toBeVisible();
    await expect(selector.locator("option")).toHaveText([
      "Dark",
      "Light",
      "Auto",
    ]);
    await expect(selector).toHaveValue("dark");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await expect(page.locator("starlight-theme-select")).toHaveCount(1);
    await expect(page.locator("[data-theme-selector], [data-mode-selector]"))
      .toHaveCount(0);

    await selector.selectOption("light");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    expect(await page.evaluate(() => localStorage.getItem("starlight-theme")))
      .toBe(
        "light",
      );
  });
});

function buildDocsiteUrl(path: string): string {
  if (!docsiteServer) {
    throw new Error("Docsite server is unavailable");
  }
  return docsiteServer.buildUrl(path);
}
