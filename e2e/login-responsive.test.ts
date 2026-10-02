import { expect, type Locator, type Page, test } from "@playwright/test";
import { promises as fs } from "node:fs";
import path from "node:path";

const emptyStorageState = { cookies: [], origins: [] };
const screenshotDir = path.resolve(
  process.cwd(),
  "../target/e2e/login-responsive",
);
const viewports = [
  { width: 320, height: 568 },
  { width: 360, height: 800 },
  { width: 390, height: 844 },
  { width: 430, height: 932 },
  { width: 768, height: 1024 },
  { width: 900, height: 700 },
  { width: 901, height: 700 },
  { width: 1280, height: 800 },
] as const;

test.describe("responsive login layout", () => {
  test.use({ storageState: emptyStorageState });

  test.beforeAll(async () => {
    await fs.rm(screenshotDir, { recursive: true, force: true });
    await fs.mkdir(screenshotDir, { recursive: true });
  });

  for (const viewport of viewports) {
    test(
      `primary sign-in action stays reachable at ${viewport.width}x${viewport.height}`,
      async ({ page }) => {
        await installAuthConfig(page, []);
        await page.setViewportSize(viewport);
        await page.goto("/login?next=%2Fspaces%2Fdemo%2Fdashboard");

        const brand = page.getByRole("heading", { name: "Ugoite" });
        const passkey = page.locator(".loginPanel > .btn.primary");
        const recovery = page.getByRole("link", { name: /passkey/i });
        const shell = page.locator(".loginShell");
        await expect(brand).toBeVisible();
        await expect(passkey).toBeVisible();
        await expect(recovery).toBeVisible();
        await expect(passkey).toHaveAccessibleName(/passkey/i);
        await expect(page.locator(".loginPanel > .btn.primary"))
          .toHaveCount(1);
        await expect(page.locator(".loginStatement")).toHaveCount(0);
        await expect(page.locator("main aside, .loginPanel .ui-card"))
          .toHaveCount(0);

        const columns = await shell.evaluate((element) =>
          getComputedStyle(element).gridTemplateColumns.trim().split(/\s+/)
        );
        expect(columns).toHaveLength(1);

        await expectControlFits(
          passkey,
          viewport.width,
          48,
        );
        await expectControlFits(recovery, viewport.width, 44);
        const documentWidth = await page.evaluate(() =>
          document.documentElement.scrollWidth
        );
        expect(documentWidth).toBeLessThanOrEqual(viewport.width + 1);
        await page.screenshot({
          path: path.join(
            screenshotDir,
            `login-${viewport.width}x${viewport.height}.png`,
          ),
          fullPage: true,
        });
      },
    );
  }

  test(
    "one configured OIDC provider follows passkey and precedes recovery",
    async ({ page }) => {
      await installAuthConfig(page, [
        {
          provider_id: "provider-one",
          issuer: "https://identity.example/tenant",
          client_id: "client-one",
        },
      ]);
      await page.setViewportSize({ width: 390, height: 844 });
      await page.goto("/login");

      const passkey = page.locator(".loginPanel > .btn.primary");
      const provider = page.getByRole("button", {
        name: /identity\.example\/tenant/,
      });
      const recovery = page.getByRole("link", { name: /passkey/i });
      await expect(passkey).toBeVisible();
      await expect(provider).toBeVisible();
      await expect(recovery).toBeVisible();
      await expectControlFits(passkey, 390, 48);
      await expectControlFits(provider, 390, 48);
      const order = await page.locator(".loginPanel").evaluate((panel) => {
        const all = [...panel.querySelectorAll("*")];
        return [
          panel.querySelector(".btn.primary"),
          panel.querySelector(".btn.tonal"),
          panel.querySelector(".loginLink"),
        ].map((element) => all.indexOf(element as Element));
      });
      expect(order[0]).toBeLessThan(order[1]);
      expect(order[1]).toBeLessThan(order[2]);
      await page.screenshot({
        path: path.join(screenshotDir, "login-oidc-390x844.png"),
        fullPage: true,
      });
    },
  );

  test("long OIDC labels wrap and keep recovery after the providers", async ({ page }) => {
    await installAuthConfig(page, [
      {
        provider_id: "provider-a",
        issuer: "https://identity.example/organizations/very-long-tenant-name",
        client_id: "client-a",
      },
      {
        provider_id: "provider-b",
        issuer: "https://identity-two.example/another-long-tenant-name",
        client_id: "client-b",
      },
    ]);
    await page.setViewportSize({ width: 320, height: 568 });
    await page.goto("/login");

    const actions = page.locator(".loginPanel > button");
    await expect(actions).toHaveCount(3);
    await expect(actions.nth(0)).toHaveAccessibleName(/passkey/i);
    await expect(actions.nth(1)).toHaveAccessibleName(/identity\.example/);
    await expect(actions.nth(2)).toHaveAccessibleName(/identity-two\.example/);
    const recovery = page.getByRole("link", { name: /passkey/i });
    await expectControlFits(actions.nth(1), 320, 48);
    await expectControlFits(actions.nth(2), 320, 48);
    await expect(recovery).toBeVisible();
    const order = await page.locator(".loginPanel").evaluate((panel) => {
      const elements = [
        panel.querySelector(".btn.primary"),
        ...panel.querySelectorAll(".btn.tonal"),
        panel.querySelector(".loginLink"),
      ];
      const all = [...panel.querySelectorAll("*")];
      return elements.map((element) => all.indexOf(element as Element));
    });
    expect(order).toHaveLength(4);
    expect(
      order.every((index, position) =>
        position === 0 || order[position - 1] < index
      ),
    )
      .toBe(true);
    await page.screenshot({
      path: path.join(screenshotDir, "login-oidc-320x568.png"),
      fullPage: true,
    });
  });

  test("REQ-FE-069: login screenshot is anchored to brand, primary action, and state structure", async ({ page }) => {
    await installAuthConfig(page, []);
    await page.setViewportSize({ width: 320, height: 568 });
    await page.goto("/login");

    const brand = page.getByRole("heading", { name: "Ugoite" });
    const primary = page.locator(".loginPanel > .btn.primary");
    await expect(brand).toBeVisible();
    await expect(primary).toHaveCount(1);
    await expect(primary).toHaveAccessibleName(/passkey/i);
    await expect(page.getByRole("link", { name: /passkey/i })).toBeVisible();
    await expect(page.locator(".loginStatement")).toHaveCount(0);
    await expect(page.locator("main aside, .loginPanel .ui-card"))
      .toHaveCount(0);
    await expect.poll(() =>
      page.evaluate(() => document.documentElement.scrollWidth)
    ).toBeLessThanOrEqual(320);
    await page.screenshot({
      path: path.join(screenshotDir, "login-structure-320x568.png"),
      fullPage: true,
    });

    await page.route("**/api/auth/passkey/start", (route) =>
      route.fulfill({
        status: 401,
        contentType: "application/json",
        body: JSON.stringify({ message: "Passkey unavailable" }),
      }));
    await primary.click();
    await expect(page.getByRole("alert")).toBeVisible();
    await expect(primary).toHaveCount(1);
    await expect(primary).toHaveAccessibleName("Try again");
    await page.screenshot({
      path: path.join(screenshotDir, "login-error-state-320x568.png"),
      fullPage: true,
    });
  });

  test("REQ-FE-069: exposes loading and configuration failure with a keyboard retry", async ({ page }) => {
    let configRequests = 0;
    await page.route("**/api/auth/config", (route) => {
      configRequests++;
      if (configRequests === 1) {
        return new Promise<void>((resolve) => setTimeout(resolve, 200))
          .then(() =>
            route.fulfill({
              status: 503,
              contentType: "application/json",
              body: JSON.stringify({
                message: "Authentication configuration unavailable",
              }),
            })
          );
      }
      return route.fulfill({
        contentType: "application/json",
        body: JSON.stringify({
          status: "active",
          node_id: "login-responsive-test",
          issuer: "http://localhost",
          rp_id: "localhost",
          passkey: true,
          oidc: false,
        }),
      });
    });
    await page.setViewportSize({ width: 320, height: 568 });
    await page.goto("/login");

    await expect(page.getByRole("heading", { name: "Ugoite" })).toBeVisible();
    await expect(page.getByRole("status")).toHaveText("Loading…");
    await expect(page.getByRole("alert")).toHaveText(
      "Sign-in options are unavailable.",
    );
    const retry = page.locator(".loginPanel > .btn.primary");
    await expect(retry).toHaveAccessibleName("Retry");
    await page.keyboard.press("Tab");
    await expect(retry).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(page.locator(".loginPanel > .btn.primary"))
      .toHaveAccessibleName(/passkey/i);
    expect(configRequests).toBe(2);
  });

  test("REQ-FE-069: exposes authentication failure and retry as accessible state", async ({ page }) => {
    await installAuthConfig(page, []);
    let attempts = 0;
    await page.route("**/api/auth/passkey/start", (route) =>
      route.fulfill({
        status: 401,
        contentType: "application/json",
        body: JSON.stringify({ message: "Passkey unavailable" }),
      }).then(() => attempts++));
    await page.setViewportSize({ width: 320, height: 568 });
    await page.goto("/login");
    const primary = page.locator(".loginPanel > .btn.primary");
    await expect(primary).toBeFocused();
    await page.keyboard.press("Enter");

    await expect(page.getByRole("alert")).toHaveText("Sign-in failed.");
    await expect(page.getByRole("alert")).toHaveCount(1);
    await expect(primary).toHaveAccessibleName("Try again");
    await expect(primary).toBeFocused();
    await expect(page.locator(".loginError details")).not.toHaveAttribute(
      "open",
      "",
    );
    await expect(page.getByRole("link", { name: /passkey/i })).toBeVisible();
    await primary.click();
    await expect.poll(() => attempts).toBe(2);
  });
});

async function installAuthConfig(
  page: Page,
  providers: Array<{ provider_id: string; issuer: string; client_id: string }>,
): Promise<void> {
  await page.route("**/api/auth/config", (route) =>
    route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        status: "active",
        node_id: "login-responsive-test",
        issuer: "http://localhost",
        rp_id: "localhost",
        passkey: true,
        oidc: providers.length > 0,
      }),
    }));
  await page.route("**/api/auth/oidc/providers", (route) =>
    route.fulfill({
      contentType: "application/json",
      body: JSON.stringify(providers),
    }));
}

async function expectControlFits(
  locator: Locator,
  viewportWidth: number,
  minimumHeight: number,
): Promise<void> {
  const rect = await locator.boundingBox();
  expect(rect).not.toBeNull();
  expect(rect!.x).toBeGreaterThanOrEqual(0);
  expect(rect!.x + rect!.width).toBeLessThanOrEqual(viewportWidth + 1);
  expect(rect!.width).toBeGreaterThanOrEqual(44);
  expect(rect!.height).toBeGreaterThanOrEqual(minimumHeight);
}
