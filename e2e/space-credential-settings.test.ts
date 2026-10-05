import { expect, test } from "@playwright/test";
import {
  getDefaultSpaceId,
  getFrontendUrl,
  waitForServers,
} from "./lib/client.ts";

test.describe("Space credential settings browser acceptance", () => {
  let spaceId = "";

  test.beforeAll(async ({ request }) => {
    await waitForServers(request);
    spaceId = await getDefaultSpaceId(request);
  });

  const credentialSettingsUrl = (tab: string) =>
    getFrontendUrl(
      `/spaces/${spaceId}/settings?section=credentials&tab=${tab}`,
    );

  test("switching credential tabs preserves the Space settings section", async ({ page }) => {
    await page.goto(credentialSettingsUrl("sessions"), {
      waitUntil: "networkidle",
    });

    const sessionsTab = page.getByRole("tab", { name: "Sessions" });
    await expect(sessionsTab).toHaveAttribute("aria-selected", "true");
    await expect(page.locator("#credential-panel-sessions")).toBeVisible();

    await page.getByRole("tab", { name: "Passkeys" }).click();
    await expect(page).toHaveURL(/section=credentials/);
    await expect(page).toHaveURL(/tab=passkeys/);
    await expect(page.getByRole("tab", { name: "Passkeys" }))
      .toHaveAttribute("aria-selected", "true");
    await expect(page.locator("#credential-panel-passkeys")).toBeVisible();

    await page.getByRole("tab", { name: "Audit Log" }).click();
    await expect(page).toHaveURL(/section=credentials/);
    await expect(page).toHaveURL(/tab=audit/);
    await expect(page.locator("#credential-panel-audit")).toBeVisible();
  });

  test("credential tabs are keyboard reachable and operable", async ({ page }) => {
    await page.goto(credentialSettingsUrl("passkeys"), {
      waitUntil: "networkidle",
    });

    const sessionsTab = page.getByRole("tab", { name: "Sessions" });
    await sessionsTab.focus();
    await expect(sessionsTab).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(sessionsTab).toHaveAttribute("aria-selected", "true");
    await expect(page).toHaveURL(/section=credentials/);
    await expect(page).toHaveURL(/tab=sessions/);
    await expect(page.locator("#credential-panel-sessions")).toBeVisible();
  });

  test("credential settings stay usable at narrow mobile width", async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 800 });
    await page.goto(credentialSettingsUrl("sessions"), {
      waitUntil: "networkidle",
    });

    await expect(page.getByRole("tab", { name: "Sessions" })).toBeVisible();
    const overflow = await page.evaluate(
      () =>
        document.documentElement.scrollWidth -
        document.documentElement.clientWidth,
    );
    expect(overflow).toBeLessThanOrEqual(1);

    await page.getByRole("tab", { name: "Passkeys" }).click();
    await expect(page).toHaveURL(/section=credentials/);
    await expect(page.locator("#credential-panel-passkeys")).toBeVisible();
  });

  test("credential settings stay usable at 200% zoom", async ({ page }) => {
    await page.goto(credentialSettingsUrl("sessions"), {
      waitUntil: "networkidle",
    });

    // Closest scriptable equivalent of browser zoom in Chromium.
    await page.evaluate(() => {
      document.body.style.zoom = "200%";
    });
    const overflow = await page.evaluate(
      () =>
        document.documentElement.scrollWidth -
        document.documentElement.clientWidth,
    );
    expect(overflow).toBeLessThanOrEqual(1);

    const sessionsTab = page.getByRole("tab", { name: "Sessions" });
    await expect(sessionsTab).toBeVisible();
    await sessionsTab.click();
    await expect(sessionsTab).toHaveAttribute("aria-selected", "true");
    await expect(page).toHaveURL(/section=credentials/);
  });

  test("the independent /settings/security route remains usable", async ({ page }) => {
    await page.goto(getFrontendUrl("/settings/security?tab=sessions"), {
      waitUntil: "networkidle",
    });

    await expect(page.getByRole("heading", { name: "Settings" }))
      .toBeVisible();
    const sessionsTab = page.getByRole("tab", { name: "Sessions" });
    await expect(sessionsTab).toHaveAttribute("aria-selected", "true");
    await expect(page.locator("#credential-panel-sessions")).toBeVisible();

    await page.getByRole("tab", { name: "Passkeys" }).click();
    await expect(page.getByRole("tab", { name: "Passkeys" }))
      .toHaveAttribute("aria-selected", "true");
    await expect(page.locator("#credential-panel-passkeys")).toBeVisible();
  });
});
