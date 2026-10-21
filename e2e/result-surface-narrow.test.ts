import {
  type APIRequestContext,
  expect,
  type Locator,
  type Page,
  test,
} from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";

type FormFieldSummary = {
  sql_column?: string;
};

type FormSummary = {
  id?: string;
  name?: string;
  sql_relation?: string;
  fields?: Record<string, FormFieldSummary>;
};

type NarrowSeed = {
  spaceId: string;
  singleColumnSqlId: string;
  manyColumnSqlId: string;
  alphaValues: string[];
};

const MANY_COLUMN_COUNT = 12;

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

async function createSpace(request: APIRequestContext): Promise<string> {
  const suffix = `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
  const response = await request.post(getBackendUrl("/spaces"), {
    data: {
      slug: `result-narrow-${suffix}`,
      name: `Result Narrow ${suffix}`,
    },
  });
  expect([200, 201]).toContain(response.status());
  const result = (await response.json()) as { space_uid?: string };
  expect(result.space_uid).toBeTruthy();
  return result.space_uid!;
}

async function seedNarrowSurfaces(
  request: APIRequestContext,
): Promise<NarrowSeed> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `NarrowSurface${suffix}`;
  const alphaValues = Array.from(
    { length: 3 },
    (_, index) =>
      `Narrow Alpha ${suffix} ${String(index + 1).padStart(3, "0")}`,
  );

  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Alpha\n\n## Beta\n`,
        fields: {
          Alpha: { type: "string", required: true },
          Beta: { type: "string", required: false },
        },
      },
    },
  );
  expect([200, 201]).toContain(formResponse.status());

  const formsResponse = await request.get(
    getBackendUrl(`/spaces/${spaceId}/forms`),
  );
  expect(formsResponse.ok()).toBe(true);
  const forms = (await formsResponse.json()) as FormSummary[];
  const form = forms.find((candidate) => candidate.name === formName);
  expect(form?.id).toBeTruthy();
  expect(form?.sql_relation).toBeTruthy();
  const alphaField = form!.fields?.Alpha;
  const betaField = form!.fields?.Beta;
  expect(alphaField?.sql_column).toBeTruthy();
  expect(betaField?.sql_column).toBeTruthy();

  for (const alpha of alphaValues) {
    const entryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      {
        data: {
          form: formName,
          fields: { Alpha: alpha, Beta: `Narrow Beta ${alpha}` },
        },
      },
    );
    expect(entryResponse.status()).toBe(201);
  }

  const relation = sqlIdentifier(form!.sql_relation!);
  const alphaColumn = sqlIdentifier(alphaField!.sql_column!);
  const betaColumn = sqlIdentifier(betaField!.sql_column!);
  const totalOrder = `ORDER BY ${alphaColumn} ASC, _ugoite_id`;

  const singleColumnSql = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name: `Narrow single column ${suffix}`,
        kind: "user-query",
        sql: `SELECT ${alphaColumn} AS label FROM ${relation} ${totalOrder}`,
        variables: [],
      },
    },
  );
  expect([200, 201]).toContain(singleColumnSql.status());
  const singleColumn = (await singleColumnSql.json()) as { id?: string };
  expect(singleColumn.id).toBeTruthy();

  const manyColumns = Array.from(
    { length: MANY_COLUMN_COUNT },
    (_, index) =>
      `${index % 2 === 0 ? alphaColumn : betaColumn} AS c${
        String(index + 1).padStart(2, "0")
      }`,
  ).join(", ");
  const manyColumnSql = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name: `Narrow many column ${suffix}`,
        kind: "user-query",
        sql: `SELECT ${manyColumns} FROM ${relation} ${totalOrder}`,
        variables: [],
      },
    },
  );
  expect([200, 201]).toContain(manyColumnSql.status());
  const manyColumn = (await manyColumnSql.json()) as { id?: string };
  expect(manyColumn.id).toBeTruthy();

  return {
    spaceId,
    singleColumnSqlId: singleColumn.id!,
    manyColumnSqlId: manyColumn.id!,
    alphaValues,
  };
}

async function expectNoPageHorizontalOverflow(page: Page): Promise<void> {
  const sizes = await page.evaluate(() => ({
    documentWidth: document.documentElement.scrollWidth,
    viewportWidth: document.documentElement.clientWidth,
  }));
  expect(sizes.documentWidth).toBeLessThanOrEqual(sizes.viewportWidth + 1);
}

async function expectViewportRectWithinViewport(
  page: Page,
  viewport: Locator,
): Promise<void> {
  const box = await viewport.boundingBox();
  expect(box).toBeTruthy();
  const innerWidth = await page.evaluate(() => window.innerWidth);
  expect(box!.x).toBeGreaterThanOrEqual(-1);
  expect(box!.x + box!.width).toBeLessThanOrEqual(innerWidth + 1);
}

async function expectNoGiantFixedMinWidth(
  table: Locator,
  viewportWidth: number,
): Promise<void> {
  const minWidth = await table.evaluate(
    (element) => getComputedStyle(element).minWidth,
  );
  if (minWidth === "none" || minWidth === "0px" || minWidth === "auto") return;
  const match = minWidth.match(/^([\d.]+)px$/);
  expect(match, `unexpected table min-width: ${minWidth}`).toBeTruthy();
  expect(Number(match![1])).toBeLessThanOrEqual(viewportWidth + 1);
}

async function expectLocatorWithinViewport(
  page: Page,
  target: Locator,
): Promise<void> {
  const box = await target.boundingBox();
  expect(box).toBeTruthy();
  const innerWidth = await page.evaluate(() => window.innerWidth);
  expect(box!.x).toBeGreaterThanOrEqual(-1);
  expect(box!.x + box!.width).toBeLessThanOrEqual(innerWidth + 1);
}

test.describe("Result surface narrow viewports", () => {
  let seed: NarrowSeed;

  test.beforeAll(async ({ request }) => {
    test.setTimeout(180_000);
    await waitForServers(request);
    seed = await seedNarrowSurfaces(request);
  });

  test("REQ-FE-068: Saved SQL 1-column keeps Count operable at narrow widths", async ({ page }) => {
    // Mitase evidence: REQ-FE-068#criterion.local-horizontal-overflow.
    test.setTimeout(120_000);
    for (const width of [320, 390]) {
      await page.setViewportSize({ width, height: 800 });
      await page.goto(
        getFrontendUrl(
          `/spaces/${seed.spaceId}/sql/${seed.singleColumnSqlId}/run`,
        ),
        { waitUntil: "domcontentloaded" },
      );

      const table = page.getByRole("table");
      await expect(table).toBeVisible();
      await expect(table.locator("tbody tr")).toHaveCount(
        seed.alphaValues.length,
      );
      for (const alpha of seed.alphaValues) {
        await expect(table).toContainText(alpha);
      }

      await expectNoPageHorizontalOverflow(page);
      const viewport = page.locator(".result-table-viewport");
      await expect(viewport).toHaveCount(1);
      await expectViewportRectWithinViewport(page, viewport);
      await expectNoGiantFixedMinWidth(table, width);

      const countButton = page.getByRole("button", { name: "Count rows" });
      await expect(countButton).toBeVisible();
      await expectLocatorWithinViewport(page, countButton);
      await countButton.click();
      await expect(page.getByText(
        new RegExp(`${seed.alphaValues.length} rows`),
      )).toBeVisible();
    }
  });

  test("REQ-FE-068: Saved SQL many-column localizes overflow in the result viewport", async ({ page }) => {
    // Mitase evidence: REQ-FE-068#criterion.local-horizontal-overflow.
    test.setTimeout(120_000);
    for (const width of [320, 390]) {
      await page.setViewportSize({ width, height: 800 });
      await page.goto(
        getFrontendUrl(
          `/spaces/${seed.spaceId}/sql/${seed.manyColumnSqlId}/run`,
        ),
        { waitUntil: "domcontentloaded" },
      );

      const table = page.getByRole("table");
      await expect(table).toBeVisible();
      await expect(table.locator("thead th")).toHaveCount(MANY_COLUMN_COUNT);
      await expect(table.locator("tbody tr")).toHaveCount(
        seed.alphaValues.length,
      );

      await expectNoPageHorizontalOverflow(page);
      const viewport = page.locator(".result-table-viewport");
      await expect(viewport).toHaveCount(1);
      await expectViewportRectWithinViewport(page, viewport);
      const metrics = await viewport.evaluate((element) => ({
        clientWidth: element.clientWidth,
        scrollWidth: element.scrollWidth,
      }));
      expect(metrics.scrollWidth).toBeGreaterThan(metrics.clientWidth);

      const countButton = page.getByRole("button", { name: "Count rows" });
      await expectLocatorWithinViewport(page, countButton);

      await viewport.evaluate((element) => {
        element.scrollLeft = element.scrollWidth;
      });
      await expectNoPageHorizontalOverflow(page);
      await expectLocatorWithinViewport(page, countButton);
      await countButton.click();
      await expect(page.getByText(
        new RegExp(`${seed.alphaValues.length} rows`),
      )).toBeVisible();
      await expectNoPageHorizontalOverflow(page);
    }
  });

  test("REQ-FE-068: SQL table reflows at the 640px CSS viewport used by 200% zoom", async ({ page }) => {
    // Mitase evidence: REQ-FE-068#criterion.local-horizontal-overflow.
    // A 1280px desktop viewport at 200% browser zoom has a 640 CSS-pixel
    // layout viewport; Playwright models that reflow width directly.
    test.setTimeout(120_000);
    await page.setViewportSize({ width: 640, height: 800 });
    await page.goto(
      getFrontendUrl(
        `/spaces/${seed.spaceId}/sql/${seed.manyColumnSqlId}/run`,
      ),
      { waitUntil: "domcontentloaded" },
    );

    const table = page.getByRole("table", { name: "SQL results" });
    const viewport = page.getByRole("region", { name: "SQL results" });
    await expect(table).toBeVisible();
    await expect(viewport).toHaveAttribute("tabindex", "0");
    await expectNoPageHorizontalOverflow(page);
    await expectViewportRectWithinViewport(page, viewport);
    const metrics = await viewport.evaluate((element) => ({
      clientWidth: element.clientWidth,
      scrollWidth: element.scrollWidth,
    }));
    expect(metrics.scrollWidth).toBeGreaterThan(metrics.clientWidth);
    await viewport.focus();
    await expect(viewport).toBeFocused();
    await page.keyboard.press("ArrowRight");
    await expect.poll(() => viewport.evaluate((element) => element.scrollLeft))
      .toBeGreaterThan(0);
    const cellStyle = await viewport.locator("td.result-table-cell > span")
      .first().evaluate((element) => {
        const style = getComputedStyle(element);
        return {
          textOverflow: style.textOverflow,
          whiteSpace: style.whiteSpace,
        };
      });
    expect(cellStyle).toEqual({
      textOverflow: "ellipsis",
      whiteSpace: "nowrap",
    });
  });
});
