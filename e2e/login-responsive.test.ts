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
      `Passkey-only login stays reachable at ${viewport.width}x${viewport.height}`,
      async ({ page }) => {
        await installAuthConfig(page, []);
        await page.setViewportSize(viewport);
        await page.goto("/login?next=%2Fspaces%2Fdemo%2Fdashboard");

        const passkey = page.getByRole("button", {
          name: "Sign in with a passkey",
        });
        const recovery = page.getByRole("link", { name: "Lost your Passkey?" });
        const shell = page.locator(".loginShell");
        await expect(passkey).toBeVisible();
        await expect(recovery).toBeVisible();
        await expect(page.getByRole("complementary", {
          name: "Ugoite principles",
        })).toBeVisible();

        const columns = await shell.evaluate((element) =>
          getComputedStyle(element).gridTemplateColumns.trim().split(/\s+/)
        );
        expect(columns).toHaveLength(viewport.width <= 900 ? 1 : 2);
        const principlesStyle = await page.locator(".loginStatement").evaluate(
          (element) => {
            const style = getComputedStyle(element);
            return {
              display: style.display,
              fontSize: Number.parseFloat(style.fontSize),
              backgroundImage: style.backgroundImage,
            };
          },
        );
        if (viewport.width <= 900) {
          expect(principlesStyle.fontSize).toBeLessThanOrEqual(14);
          expect(principlesStyle.backgroundImage).toBe("none");
          const passkeyRect = await passkey.boundingBox();
          const panelContentWidth = await page.locator(".loginPanel").evaluate(
            (panel) => {
              const style = getComputedStyle(panel);
              return panel.clientWidth -
                Number.parseFloat(style.paddingLeft) -
                Number.parseFloat(style.paddingRight);
            },
          );
          expect(passkeyRect!.width).toBeGreaterThanOrEqual(
            panelContentWidth - 1,
          );
        } else {
          expect(principlesStyle.backgroundImage).toContain("linear-gradient");
        }

        await expectControlFits(
          passkey,
          viewport.width,
          viewport.width <= 900 ? 48 : 44,
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

      const passkey = page.getByRole("button", {
        name: "Sign in with a passkey",
      });
      const provider = page.getByRole("button", {
        name: "Continue with identity.example/tenant",
      });
      const recovery = page.getByRole("link", { name: "Lost your Passkey?" });
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
    await expect(actions.nth(0)).toHaveAccessibleName(
      "Sign in with a passkey",
    );
    await expect(actions.nth(1)).toHaveAccessibleName(
      "Continue with identity.example/organizations/very-long-tenant-name",
    );
    await expect(actions.nth(2)).toHaveAccessibleName(
      "Continue with identity-two.example/another-long-tenant-name",
    );
    const recovery = page.getByRole("link", { name: "Lost your Passkey?" });
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

  test("shows a single inline error while keeping sign-in and recovery available", async ({ page }) => {
    await installAuthConfig(page, []);
    await page.route("**/api/auth/passkey/start", (route) =>
      route.fulfill({
        status: 401,
        contentType: "application/json",
        body: JSON.stringify({ message: "Passkey unavailable" }),
      }));
    await page.setViewportSize({ width: 320, height: 568 });
    await page.goto("/login");
    await page.getByRole("button", { name: "Sign in with a passkey" })
      .click();

    await expect(page.getByRole("alert")).toHaveText("Passkey unavailable");
    await expect(page.getByRole("alert")).toHaveCount(1);
    await expect(page.getByRole("button", {
      name: "Sign in with a passkey",
    })).toBeVisible();
    await expect(page.getByRole("link", { name: "Lost your Passkey?" }))
      .toBeVisible();
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
