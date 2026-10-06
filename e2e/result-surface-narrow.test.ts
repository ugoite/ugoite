import {
  type APIRequestContext,
  expect,
  type Locator,
  type Page,
  test,
} from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";

type FormFieldSummary = {
  id?: number;
  sql_column?: string;
  query_capability?: { field?: { field_id?: number } };
};

type FormSummary = {
  id?: string;
  name?: string;
  sql_relation?: string;
  fields?: Record<string, FormFieldSummary>;
};

type CompositionSaveResponse = {
  composition_id: string;
  revision_id: string;
};

type NarrowSeed = {
  spaceId: string;
  compositionOneColumnId: string;
  compositionOneColumnRevision: string;
  compositionTwoColumnId: string;
  compositionTwoColumnRevision: string;
  singleColumnSqlId: string;
  manyColumnSqlId: string;
  alphaValues: string[];
};

const MANY_COLUMN_COUNT = 12;

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

function fieldId(field: FormFieldSummary | undefined): number {
  const resolved = field?.query_capability?.field?.field_id ?? field?.id;
  expect(resolved).toBeTruthy();
  return resolved!;
}

function buildEntryQueryYaml(
  name: string,
  formId: string,
  fieldIds: number[],
  pageLimit?: number,
): string {
  const lines = [
    "format: ugoite.composition",
    "format_version: 1",
    `name: ${name}`,
    "kind: dashboard",
    "tags: []",
    "spec:",
    "  sources:",
    "    - id: narrow_rows",
    "      kind: entry_query",
    `      form_id: "${formId}"`,
    "      field_schema:",
    ...fieldIds.map((field) =>
      `        - field_id: ${field}\n          field_type: string`
    ),
    "      query:",
    ...(pageLimit === undefined ? [] : [`        page_limit: ${pageLimit}`]),
    "        projection:",
    "          kind: fields",
    `          fields: [${fieldIds.join(", ")}]`,
    "  components:",
    "    - id: narrow_table",
    "      kind: table",
    "      source: narrow_rows",
    "  sections:",
    "    - id: main",
    "      components: [narrow_table]",
    "",
  ];
  return lines.join("\n");
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

async function saveComposition(
  request: APIRequestContext,
  spaceId: string,
  yaml: string,
): Promise<CompositionSaveResponse> {
  const response = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions`),
    {
      headers: { "Idempotency-Key": crypto.randomUUID() },
      data: { yaml },
    },
  );
  expect(response.status()).toBe(201);
  const saved = (await response.json()) as CompositionSaveResponse;
  expect(saved.composition_id).toBeTruthy();
  expect(saved.revision_id).toBeTruthy();
  return saved;
}

async function seedNarrowSurfaces(
  request: APIRequestContext,
): Promise<NarrowSeed> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `NarrowSurface${suffix}`;
  const alphaValues = [1, 2, 3].map((index) =>
    `Narrow Alpha ${suffix} ${index}`
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
  const alphaFieldId = fieldId(alphaField);
  const betaFieldId = fieldId(betaField);

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
  const totalOrder = `ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 50`;

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

  const oneColumn = await saveComposition(
    request,
    spaceId,
    buildEntryQueryYaml(
      `Narrow one column ${suffix}`,
      form!.id!,
      [alphaFieldId],
    ),
  );
  const twoColumn = await saveComposition(
    request,
    spaceId,
    buildEntryQueryYaml(
      `Narrow two column ${suffix}`,
      form!.id!,
      [alphaFieldId, betaFieldId],
      2,
    ),
  );

  return {
    spaceId,
    compositionOneColumnId: oneColumn.composition_id,
    compositionOneColumnRevision: oneColumn.revision_id,
    compositionTwoColumnId: twoColumn.composition_id,
    compositionTwoColumnRevision: twoColumn.revision_id,
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
    test.setTimeout(120_000);
    await waitForServers(request);
    seed = await seedNarrowSurfaces(request);
  });

  test("REQ-FE-068: Composition 1-column fits 320px and 390px without page overflow", async ({ page }) => {
    // Mitase evidence: REQ-FE-068#criterion.local-horizontal-overflow.
    test.setTimeout(120_000);
    for (const width of [320, 390]) {
      await page.setViewportSize({ width, height: 800 });
      await page.goto(
        getFrontendUrl(
          `/spaces/${seed.spaceId}/compositions/${seed.compositionOneColumnId}/${seed.compositionOneColumnRevision}`,
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
      const metrics = await viewport.evaluate((element) => ({
        clientWidth: element.clientWidth,
        scrollWidth: element.scrollWidth,
      }));
      expect(metrics.scrollWidth).toBeLessThanOrEqual(metrics.clientWidth + 1);
      await expectNoGiantFixedMinWidth(table, width);
    }
  });

  test("REQ-FE-068: Composition 2-column confines overflow to the result viewport", async ({ page }) => {
    // Mitase evidence: REQ-FE-068#criterion.local-horizontal-overflow.
    test.setTimeout(120_000);
    for (const width of [320, 390]) {
      await page.setViewportSize({ width, height: 800 });
      await page.goto(
        getFrontendUrl(
          `/spaces/${seed.spaceId}/compositions/${seed.compositionTwoColumnId}/${seed.compositionTwoColumnRevision}`,
        ),
        { waitUntil: "domcontentloaded" },
      );

      const table = page.getByRole("table");
      await expect(table).toBeVisible();
      await expect(table.locator("thead th")).toHaveCount(2);
      await expect(table.locator("tbody tr")).toHaveCount(2);

      await expectNoPageHorizontalOverflow(page);
      const viewport = page.locator(".result-table-viewport");
      await expect(viewport).toHaveCount(1);
      await expectViewportRectWithinViewport(page, viewport);
      await expectNoGiantFixedMinWidth(table, width);

      const next = page.getByRole("button", { name: "Next", exact: true });
      const previous = page.getByRole("button", {
        name: "Previous",
        exact: true,
      });
      if (await next.count() > 0) {
        await expect(next).toBeEnabled();
        await next.click();
        await expect(table.locator("tbody tr")).toHaveCount(1);
        await expectNoPageHorizontalOverflow(page);
        await expectViewportRectWithinViewport(page, viewport);
        await expect(previous).toBeEnabled();
        await previous.click();
        await expect(table.locator("tbody tr")).toHaveCount(2);
        await expectNoPageHorizontalOverflow(page);
      }
    }
  });

  test("REQ-FE-068: Saved SQL 1-column keeps Count and Save-as-tool operable at narrow widths", async ({ page }) => {
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

      const saveAsTool = page.getByRole("button", { name: "Save as tool" });
      await expect(saveAsTool).toBeVisible();
      await expectLocatorWithinViewport(page, saveAsTool);

      const countButton = page.getByRole("button", { name: "Count rows" });
      await expect(countButton).toBeVisible();
      await expectLocatorWithinViewport(page, countButton);
      await countButton.click();
      await expect(page.getByText(
        new RegExp(`${seed.alphaValues.length} rows`),
      )).toBeVisible();

      await saveAsTool.click();
      // Save-as-tool seeds the Studio instead of a dialog: the Studio
      // opens prefilled with the Saved SQL source and stays save-ready.
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${seed.spaceId}/compositions/new$`),
      );
      await expect(
        page.getByRole("button", { name: "Save", exact: true }),
      ).toBeEnabled();
      await expectNoPageHorizontalOverflow(page);
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
      const saveAsTool = page.getByRole("button", { name: "Save as tool" });
      await expectLocatorWithinViewport(page, countButton);
      await expectLocatorWithinViewport(page, saveAsTool);

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
});
