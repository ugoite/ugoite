import { expect, test } from "@playwright/test";
import {
  getDefaultSpaceId,
  getFrontendUrl,
  waitForServers,
} from "./lib/client.ts";

test.describe("QRY-02 visible result rows", () => {
  test("keeps a 1,000-entry page viewport-sized and keyboard navigable", async ({ page, request }) => {
    await waitForServers(request);
    const spaceId = await getDefaultSpaceId(request);
    const fixture = Array.from({ length: 1_000 }, (_, index) => ({
      id: `019c1234-5678-7abc-8def-${index.toString(16).padStart(12, "0")}`,
      form_id: "019c1234-5678-7abc-8def-000000000001",
      revision_id: "019c1234-5678-7abc-8def-000000000002",
      created_at_micros: 1_772_960_000_000_000,
      updated_at_micros: 1_772_963_000_000_000,
      preview: String(index),
    }));

    await page.route(
      `**/api/spaces/${spaceId}/entries/query`,
      async (route) => {
        if (route.request().method() !== "POST") return await route.continue();
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({ rows: fixture, has_more: false }),
        });
      },
    );
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto(getFrontendUrl(`/spaces/${spaceId}/search`), {
      waitUntil: "domcontentloaded",
    });
    await page.getByLabel("Search keywords").fill("virtualized-results");
    const renderStartedAt = await page.evaluate(() => performance.now());
    await page.getByRole("button", { name: "Search entries" }).click();

    const table = page.locator(".entry-browser-table");
    await expect(table.locator('[data-entry-id*="000000000000"]')).toHaveCount(
      1,
    );
    const elapsedToFirstRowMs = await page.evaluate((startedAt) =>
      performance.now() - startedAt, renderStartedAt);
    const measuredRows = await table.locator("tr").count();
    const measuredDataRows = await table.locator("tbody tr[data-row-index]")
      .count();
    const usedHeapBytes = await page.evaluate(() =>
      (performance as Performance & {
        memory?: { usedJSHeapSize: number };
      }).memory?.usedJSHeapSize ?? null
    );
    console.info("QRY02_VISIBLE_ROWS_MEASUREMENT", JSON.stringify({
      elapsedToFirstRowMs,
      measuredRows,
      measuredDataRows,
      usedHeapBytes,
      viewport: { width: 1280, height: 720 },
      suppliedRows: fixture.length,
    }));

    expect(measuredDataRows).toBe(22);
    expect(measuredRows).toBe(24);
    await expect(table).toHaveAttribute("aria-rowcount", "1001");

    const scroll = page.locator(".entry-browser-table-scroll");
    await scroll.evaluate((element) => {
      const scrollable = element as HTMLDivElement;
      scrollable.scrollTop = 48 * 100;
      scrollable.dispatchEvent(new Event("scroll"));
    });
    await expect(table.getByText("94", { exact: true })).toBeVisible();
    await expect(table.getByText("0", { exact: true })).toHaveCount(0);

    await scroll.evaluate(async (element) => {
      const scrollable = element as HTMLDivElement;
      scrollable.scrollTop = 0;
      scrollable.dispatchEvent(new Event("scroll"));
      await new Promise<void>((resolve) => {
        requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
      });
    });
    const lastVisibleIndex = await scroll.evaluate((element) => {
      const bounds = element.getBoundingClientRect();
      const visibleRows = Array.from(
        element.querySelectorAll<HTMLTableRowElement>("tr[data-row-index]"),
      ).filter((row) => {
        const rowBounds = row.getBoundingClientRect();
        return rowBounds.top >= bounds.top && rowBounds.bottom <= bounds.bottom;
      });
      return Number(visibleRows.at(-1)?.dataset.rowIndex);
    });
    const lastVisibleRowAction = table.locator(
      `tbody tr[data-row-index="${lastVisibleIndex}"] button`,
    );
    const nextRowAction = table.locator(
      `tbody tr[data-row-index="${lastVisibleIndex + 1}"] button`,
    );
    await expect(lastVisibleRowAction).toBeVisible();
    await lastVisibleRowAction.focus();
    await page.keyboard.press("ArrowDown");
    await expect(nextRowAction).toBeFocused();
  });
});
