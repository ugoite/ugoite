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
    await expect(page.getByRole("heading", { name: "default" }))
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
    await expect(page.getByRole("heading", { name: "default" }))
      .toBeVisible();
  });
});
