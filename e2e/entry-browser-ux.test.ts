import { expect, test } from "@playwright/test";
import {
  getBackendUrl,
  getDefaultSpaceId,
  getFrontendUrl,
  waitForServers,
} from "./lib/client.ts";

test.describe("Form Entry browser UX", () => {
  let spaceId = "";
  let alphaForm = "";
  let betaForm = "";
  let alphaEntryId = "";
  let betaEntryId = "";
  let alphaSummary = "";

  test.beforeAll(async ({ request }) => {
    await waitForServers(request);
    spaceId = await getDefaultSpaceId(request);
    const suffix = Date.now();
    alphaForm = `EntryUxAlpha${suffix}`;
    betaForm = `EntryUxBeta${suffix}`;
    alphaSummary = `Alpha entry ${suffix}`;

    for (const name of [alphaForm, betaForm]) {
      const response = await request.post(
        getBackendUrl(`/spaces/${spaceId}/forms`),
        {
          data: {
            name,
            version: 1,
            template: `# ${name}\n\n## Summary\n\n## Notes\n`,
            fields: {
              Summary: { type: "string", required: true },
              Notes: { type: "string", required: false },
            },
          },
        },
      );
      expect(response.status()).toBe(201);
    }

    const alphaResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      {
        data: {
          form: alphaForm,
          fields: {
            Summary: alphaSummary,
            Notes: "Long content for a narrow table viewport. ".repeat(12),
          },
        },
      },
    );
    expect(alphaResponse.status()).toBe(201);
    alphaEntryId = ((await alphaResponse.json()) as { id: string }).id;

    const betaResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      {
        data: {
          form: betaForm,
          fields: {
            Summary: `Beta entry ${suffix}`,
            Notes: "Second form fixture",
          },
        },
      },
    );
    expect(betaResponse.status()).toBe(201);
    betaEntryId = ((await betaResponse.json()) as { id: string }).id;
  });

  test.afterAll(async ({ request }) => {
    for (const entryId of [alphaEntryId, betaEntryId]) {
      if (entryId) {
        await request.delete(
          getBackendUrl(`/spaces/${spaceId}/entries/${entryId}`),
        );
      }
    }
  });

  test("REQ-E2E-003: selects rows and keeps the trailing action usable on mobile", async ({ page }) => {
    test.setTimeout(90_000);
    for (const width of [320, 390]) {
      await page.setViewportSize({ width, height: 800 });
      await page.goto(
        getFrontendUrl(`/spaces/${spaceId}/forms/${alphaForm}/entries`),
        { waitUntil: "domcontentloaded" },
      );

      const table = page.getByRole("table");
      await expect(table).toBeVisible();
      const row = page.getByRole("row", { name: /Alpha entry/ });
      await expect(row).toBeVisible();
      const pageWidths = await page.evaluate(() => ({
        documentWidth: document.documentElement.scrollWidth,
        viewportWidth: document.documentElement.clientWidth,
      }));
      expect(pageWidths.documentWidth).toBeLessThanOrEqual(
        pageWidths.viewportWidth + 1,
      );

      const scroll = page.locator(".entry-browser-table-scroll");
      const scrollMetrics = await scroll.evaluate((element) => ({
        clientWidth: element.clientWidth,
        scrollWidth: element.scrollWidth,
      }));
      expect(scrollMetrics.scrollWidth).toBeGreaterThan(
        scrollMetrics.clientWidth,
      );

      const openButton = row.getByRole("button", { name: "Open entry" });
      const initialActionX = (await openButton.boundingBox())?.x;
      expect(initialActionX).toBeDefined();
      await scroll.evaluate((element) => {
        element.scrollLeft = element.scrollWidth;
      });
      await expect.poll(async () => (await openButton.boundingBox())?.x)
        .toBeCloseTo(initialActionX!, 0);

      await row.click();
      await expect(row).toHaveAttribute("aria-selected", "true");
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${spaceId}/forms/${alphaForm}/entries$`),
      );

      if (width === 390) {
        const filterButton = page.getByRole("button", { name: "Filter" });
        await filterButton.focus();
        await page.keyboard.press("Enter");
        const dialog = page.getByRole("dialog");
        await expect(dialog).toBeVisible();
        await expect(dialog.getByRole("heading")).toBeVisible();
        await page.keyboard.press("Escape");
        await expect(dialog).toHaveCount(0);
        await expect(filterButton).toBeFocused();
        await openButton.click();
      }
    }

    await expect(page).toHaveURL(
      new RegExp(`/spaces/${spaceId}/entries/${alphaEntryId}$`),
    );
    await expect(page.getByLabel("Summary")).toHaveValue(alphaSummary);
  });

  test("REQ-E2E-003: changing Forms does not retain the previous Entry rows", async ({ page }) => {
    await page.goto(
      getFrontendUrl(`/spaces/${spaceId}/forms/${alphaForm}/entries`),
      { waitUntil: "domcontentloaded" },
    );
    await expect(page.getByText(new RegExp("Alpha entry"))).toBeVisible();
    await expect(page.getByText(new RegExp("Beta entry"))).toHaveCount(0);

    await page.getByRole("link", { name: "Forms" }).first().click();
    await expect(page).toHaveURL(
      new RegExp(`/spaces/${spaceId}/forms$`),
    );
    await page.getByRole("button", { name: betaForm, exact: true }).click();
    await expect(page).toHaveURL(
      new RegExp(`/spaces/${spaceId}/forms/${betaForm}/entries$`),
    );
    await expect(page.getByText(new RegExp("Beta entry"))).toBeVisible();
    await expect(page.getByText(new RegExp("Alpha entry"))).toHaveCount(0);
  });
});
