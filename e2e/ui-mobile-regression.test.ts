import { expect, type Page, test } from "@playwright/test";
import { promises as fs } from "node:fs";
import path from "node:path";
import {
  ensureDefaultForm,
  getBackendUrl,
  getDefaultSpaceId,
  waitForServers,
} from "./lib/client.ts";
import {
  expectMobileControlFontSize,
  expectNoObjectCoercion,
} from "./lib/ui-safety.ts";

const screenshotDir = path.resolve(
  process.cwd(),
  "../target/ui-screenshots/mobile",
);
const viewports = [
  { width: 390, height: 844 },
  { width: 360, height: 800 },
] as const;

async function expectNoHorizontalOverflow(page: Page): Promise<void> {
  const overflow = await page.evaluate(() => ({
    documentWidth: document.documentElement.scrollWidth,
    viewportWidth: document.documentElement.clientWidth,
  }));
  expect(overflow.documentWidth).toBeLessThanOrEqual(
    overflow.viewportWidth + 1,
  );
}

async function expectMobileTouchTargets(page: Page): Promise<void> {
  const sizes = await page.locator(
    ".topbar .mobileMenu, .assistantPill, .accountMenu .avatar, .bottomNav a, .ui-entry-action-bar .ui-entry-tool",
  ).evaluateAll((elements) =>
    elements.map((element) => {
      const rect = element.getBoundingClientRect();
      return { height: rect.height, width: rect.width };
    })
  );

  expect(sizes.length).toBeGreaterThan(0);
  for (const size of sizes) {
    expect(size.width).toBeGreaterThanOrEqual(44);
    expect(size.height).toBeGreaterThanOrEqual(44);
  }
}

test.describe("Mobile UI regression @screenshot", () => {
  let spaceId = "";
  let entryId = "";

  test.beforeAll(async ({ request }) => {
    await waitForServers(request);
    spaceId = await getDefaultSpaceId(request);
    await ensureDefaultForm(request, spaceId);

    const response = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      {
        data: {
          form: "Entry",
          fields: { Body: "Mobile layout fixture." },
        },
      },
    );
    expect(response.status()).toBe(201);
    entryId = ((await response.json()) as { id: string }).id;
    await fs.mkdir(screenshotDir, { recursive: true });
  });

  test.afterAll(async ({ request }) => {
    if (entryId) {
      await request.delete(
        getBackendUrl(`/spaces/${spaceId}/entries/${entryId}`),
      );
    }
  });

  test("REQ-E2E-003: preserves the Space UI at 390x844", async ({ page }) => {
    test.setTimeout(120_000);
    await runMobileRegression(page, spaceId, entryId, viewports[0]);
  });

  test("REQ-E2E-003: preserves the Space UI at 360x800", async ({ page }) => {
    test.setTimeout(120_000);
    await runMobileRegression(page, spaceId, entryId, viewports[1]);
  });

  test("REQ-E2E-003: keeps one Forms workspace list across desktop and mobile viewports", async ({ page }) => {
    // Mitase evidence: REQ-E2E-003#criterion.responsive-mobile-workflows.
    test.setTimeout(120_000);
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto(`/spaces/${spaceId}/forms`, {
      waitUntil: "domcontentloaded",
    });
    await page.locator(".formsPage").waitFor({ state: "visible" });
    await expect(page.getByRole("heading", { name: "Forms" })).toBeVisible();
    await expect(page.locator(".rowListItem").first()).toBeVisible();
    await expect(page.locator(".rowList")).toBeVisible();
    await expect(page.locator(".mobileFormPicker")).toHaveCount(0);
    await expect(page.locator(".desktopFormPicker")).toHaveCount(0);
    await expect(page.locator(".formsPage select")).toHaveCount(0);
    await expectNoHorizontalOverflow(page);

    await page.setViewportSize(viewports[0]);
    await page.goto(`/spaces/${spaceId}/forms`, {
      waitUntil: "domcontentloaded",
    });
    await page.locator(".formsPage").waitFor({ state: "visible" });
    await expect(page.getByRole("heading", { name: "Forms" })).toBeVisible();
    await expect(page.locator(".rowListItem").first()).toBeVisible();
    await expect(page.locator(".rowList")).toBeVisible();
    await expect(page.locator(".mobileFormPicker")).toHaveCount(0);
    await expect(page.locator(".desktopFormPicker")).toHaveCount(0);
    await expect(page.locator(".bottomNav")).toBeVisible();
    await expectNoHorizontalOverflow(page);
    await expectNoObjectCoercion(page);
  });
});

async function runMobileRegression(
  page: Page,
  spaceId: string,
  entryId: string,
  viewport: (typeof viewports)[number],
): Promise<void> {
  await page.setViewportSize(viewport);
  const cases = [
    {
      name: "dashboard",
      path: `/spaces/${spaceId}/dashboard`,
      ready: ".bottomNav",
      assert: async () => {
        // Mitase evidence: REQ-E2E-003#criterion.responsive-mobile-workflows.
        await expect(page.getByRole("heading", { name: "Home" }))
          .toBeVisible();
        await expectMobileTouchTargets(page);
      },
    },
    {
      name: "forms",
      path: `/spaces/${spaceId}/forms`,
      ready: ".formsPage",
      assert: async () => {
        // Mitase evidence: REQ-E2E-003#criterion.responsive-mobile-workflows.
        await expect(page.getByRole("heading", { name: "Forms" }))
          .toBeVisible();
        await expect(page.locator(".rowListItem").first()).toBeVisible();
        await expect(page.locator(".mobileFormPicker")).toHaveCount(0);
        await expect(page.locator(".desktopFormPicker")).toHaveCount(0);
        await expectMobileControlFontSize(page);
      },
    },
    {
      name: "search",
      path: `/spaces/${spaceId}/search`,
      ready: "#search-keywords",
      assert: async () => {
        // Mitase evidence: REQ-E2E-003#criterion.responsive-mobile-workflows.
        await expect(page.getByLabel("Search keywords")).toBeVisible();
        await expect(page.getByRole("link", { name: "Saved" }))
          .toHaveAttribute("href", `/spaces/${spaceId}/sql`);
        await expect(page.getByRole("heading", { name: "Search history" }))
          .not.toBeAttached();
        await expect(page.locator(".topbarMore")).toHaveCount(0);
        await expectMobileControlFontSize(page);
      },
    },
    {
      name: "entries",
      path: `/spaces/${spaceId}/entries`,
      ready: ".entriesList",
      assert: async () => {
        // Mitase evidence: REQ-E2E-003#criterion.responsive-mobile-workflows.
        await expect(page.getByRole("heading", { name: "Entries" }))
          .toBeVisible();
        await expect(page.getByLabel("Filter entries")).toBeVisible();
        await expectMobileControlFontSize(page);
      },
    },
    {
      name: "files",
      path: `/spaces/${spaceId}/assets`,
      ready: ".assetInventory",
      assert: async () => {
        // Mitase evidence: REQ-E2E-003#criterion.responsive-mobile-workflows.
        await expect(page.getByRole("heading", { name: "Files" }))
          .toBeVisible();
        await expect(page.locator(".assetInventory .ui-card")).toHaveCount(0);
      },
    },
    {
      name: "settings",
      path: `/spaces/${spaceId}/settings`,
      ready: ".settingsMenuButton",
      assert: async () => {
        // Mitase evidence: REQ-E2E-003#criterion.responsive-mobile-workflows.
        // Categories hide behind the menu button on mobile; the drawer
        // reveals them and closes on selection or Escape.
        const menuButton = page.getByRole("button", {
          name: /Settings menu/,
        });
        await expect(menuButton).toBeVisible();
        await expect(page.getByRole("list", { name: "Settings" }))
          .toBeHidden();
        await menuButton.click();
        await expect(page.getByRole("list", { name: "Settings" }))
          .toBeVisible();
        await expectMobileControlFontSize(page);
        await page.keyboard.press("Escape");
        await expect(page.getByRole("list", { name: "Settings" }))
          .toBeHidden();
      },
    },
    {
      name: "history",
      path: `/spaces/${spaceId}/history`,
      ready: ".screenHead",
      assert: async () => {
        // Mitase evidence: REQ-E2E-003#criterion.responsive-mobile-workflows.
        await expect(page.getByRole("heading", { name: "Space history" }))
          .toBeVisible();
      },
    },
    {
      name: "entry",
      path: `/spaces/${spaceId}/entries/${entryId}`,
      ready: ".ui-entry-workspace",
      assert: async () => {
        // Mitase evidence: REQ-E2E-003#criterion.responsive-mobile-workflows.
        const columns = await page.locator(".ui-entry-workspace")
          .evaluate((element) =>
            getComputedStyle(element).gridTemplateColumns.trim().split(
              /\s+/,
            )
          );
        expect(columns).toHaveLength(1);
        await expect(page.locator(".ui-entry-action-bar")).toBeVisible();
        await expect(page.getByRole("link", { name: "Info" }))
          .toHaveAttribute(
            "href",
            `/spaces/${spaceId}/entries/${entryId}/info`,
          );
        await expect(page.locator(".ui-entry-mode-tabs")).toHaveCount(0);
        await expectMobileTouchTargets(page);
      },
    },
  ];

  for (const item of cases) {
    await page.goto(item.path, { waitUntil: "domcontentloaded" });
    await page.locator(item.ready).waitFor({ state: "visible" });
    await item.assert();
    await expectNoObjectCoercion(page);
    await expect(page.locator(".desktopSidebar")).toBeHidden();
    await expect(page.locator(".bottomNav")).toBeVisible();
    await expectNoHorizontalOverflow(page);
    const screenshotPath = path.join(
      screenshotDir,
      String(viewport.width),
      `${item.name}.png`,
    );
    await fs.mkdir(path.dirname(screenshotPath), { recursive: true });
    await page.screenshot({
      path: screenshotPath,
      fullPage: false,
    });
  }
}
