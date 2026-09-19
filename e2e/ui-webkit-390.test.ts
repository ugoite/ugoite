import { expect, type Page, test } from "@playwright/test";
import {
  ensureDefaultForm,
  getBackendUrl,
  getDefaultSpaceId,
  waitForServers,
} from "./lib/client.ts";

// WebKit at 390px is an automated layout check only. It is not evidence of
// a physical iPhone optical check (#2842 stays open for the device pass).
test.use({ browserName: "webkit", viewport: { width: 390, height: 844 } });

async function expectNoHorizontalOverflow(page: Page): Promise<void> {
  const overflow = await page.evaluate(() => ({
    documentWidth: document.documentElement.scrollWidth,
    viewportWidth: document.documentElement.clientWidth,
  }));
  expect(overflow.documentWidth).toBeLessThanOrEqual(
    overflow.viewportWidth + 1,
  );
}

test.describe("WebKit 390px quiet layout", () => {
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
          markdown:
            `---\nform: Entry\n---\n# WebKit 390px ${Date.now()}\n\n## Body\nWebKit layout fixture.`,
        },
      },
    );
    expect(response.status()).toBe(201);
    entryId = ((await response.json()) as { id: string }).id;
  });

  test.afterAll(async ({ request }) => {
    if (entryId) {
      await request.delete(
        getBackendUrl(`/spaces/${spaceId}/entries/${entryId}`),
      );
    }
  });

  test("keeps dashboard, entry, and history within 390px with 44px targets", async ({
    page,
  }) => {
    test.setTimeout(120_000);
    for (
      const path of [
        `/spaces/${spaceId}/dashboard`,
        `/spaces/${spaceId}/entries/${entryId}`,
        `/spaces/${spaceId}/history`,
        `/spaces/${spaceId}/assets`,
      ]
    ) {
      await page.goto(path, { waitUntil: "domcontentloaded" });
      await page.locator(".screenHead, .ui-entry-workspace").first().waitFor({
        state: "visible",
      });
      await expectNoHorizontalOverflow(page);
    }

    const sizes = await page.locator(
      ".bottomNav a, .ui-entry-action-bar .ui-entry-tool",
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
  });
});
