import { expect, test } from "@playwright/test";
import {
  ensureDefaultForm,
  getDefaultSpaceId,
  waitForServers,
} from "./lib/client.ts";

test.describe("Fixed surface palette", () => {
  let spaceId = "";

  test.beforeAll(async ({ request }) => {
    await waitForServers(request);
    spaceId = await getDefaultSpaceId(request);
    await ensureDefaultForm(request, spaceId);
  });

  test("REQ-E2E-003: the fixed surface palette follows the light system color mode", async ({ page }) => {
    // Mitase evidence: REQ-E2E-003#criterion.fixed-palette-workflows.
    // Mitase evidence: REQ-E2E-003#criterion.system-color-mode.
    await page.emulateMedia({ colorScheme: "light" });
    await page.goto(`/spaces/${spaceId}/dashboard`, {
      waitUntil: "networkidle",
    });

    await expect(page.locator("html")).toHaveAttribute(
      "data-color-mode",
      "light",
    );
    await expect(page.getByRole("link", { name: "Home" }).first())
      .toBeVisible();
    await expect(page.getByRole("heading", { name: "Home" }))
      .toBeVisible();
  });

  test("REQ-E2E-003: the fixed surface palette follows the dark system color mode", async ({ page }) => {
    // Mitase evidence: REQ-E2E-003#criterion.fixed-palette-workflows.
    // Mitase evidence: REQ-E2E-003#criterion.system-color-mode.
    await page.emulateMedia({ colorScheme: "dark" });
    await page.goto(`/spaces/${spaceId}/dashboard`, {
      waitUntil: "networkidle",
    });

    await expect(page.locator("html")).toHaveAttribute(
      "data-color-mode",
      "dark",
    );
    await expect(page.getByRole("link", { name: "Home" }).first())
      .toBeVisible();
    await expect(page.getByRole("heading", { name: "Home" }))
      .toBeVisible();
  });

  test("REQ-E2E-003: Search navigation keeps its destinations and contrast in dark mode", async ({ page }) => {
    // Mitase evidence: REQ-E2E-003#criterion.fixed-palette-workflows.
    // Mitase evidence: REQ-E2E-003#criterion.system-color-mode.
    await page.emulateMedia({ colorScheme: "dark" });
    await page.goto(`/spaces/${spaceId}/search`, {
      waitUntil: "domcontentloaded",
    });

    await expect(page.locator("html")).toHaveAttribute(
      "data-color-mode",
      "dark",
    );
    const navigation = page.getByRole("navigation", { name: "Search" });
    await expect(navigation).toBeVisible();
    await expect(page.getByLabel("Search keywords")).toBeVisible();
    await expect(page.getByRole("button", { name: "Search entries" }))
      .toBeVisible();
    await expect(navigation.getByRole("link", { name: "Files" }))
      .toHaveAttribute("href", `/spaces/${spaceId}/assets`);
    await expect(navigation.getByRole("link", { name: "Saved" }))
      .toHaveAttribute("href", `/spaces/${spaceId}/sql`);

    const contrast = await navigation.evaluate((element) => {
      const row = element.querySelector("a, button");
      if (!row) return null;
      const style = getComputedStyle(row);
      return {
        color: style.color,
        backgroundColor: style.backgroundColor,
      };
    });
    expect(contrast).not.toBeNull();
    expect(contrast?.color).toBeTruthy();
    expect(contrast?.color).not.toBe(contrast?.backgroundColor);
  });
});
