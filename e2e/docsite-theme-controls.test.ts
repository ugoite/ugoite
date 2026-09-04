import { expect, type Page, test } from "@playwright/test";
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
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });

    let selector = await expectVisibleThemeSelector(page, "dark");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await expect(page.locator("[data-theme-selector], [data-mode-selector]"))
      .toHaveCount(0);

    await selector.selectOption("light");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    expect(await page.evaluate(() => localStorage.getItem("starlight-theme")))
      .toBe(
        "light",
      );

    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(buildDocsiteUrl(docPath), { waitUntil: "networkidle" });
    await page.getByRole("button", { name: "Menu" }).click();

    selector = await expectVisibleThemeSelector(page, "dark");
    await selector.selectOption("light");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    expect(await page.evaluate(() => localStorage.getItem("starlight-theme")))
      .toBe(
        "light",
      );
  });
});

async function expectVisibleThemeSelector(page: Page, value: string) {
  // Starlight renders separate desktop/mobile pickers. Only the visible picker
  // is exposed to interaction and accessibility locators at a time.
  const selector = page.locator("starlight-theme-select select:visible");
  await expect(selector).toHaveCount(1);
  await expect(page.getByRole("combobox")).toHaveCount(1);
  await selector.focus();
  await expect(selector).toBeFocused();
  await expect(selector.locator("option")).toHaveText([
    "Dark",
    "Light",
    "Auto",
  ]);
  await expect(selector).toHaveValue(value);
  return selector;
}

function buildDocsiteUrl(path: string): string {
  if (!docsiteServer) {
    throw new Error("Docsite server is unavailable");
  }
  return docsiteServer.buildUrl(path);
}
