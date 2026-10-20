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

type CompositionSaveResponse = {
  composition_id: string;
  revision_id: string;
};

type CompositionTableReturn = {
  kind: "table";
  columns: Array<{
    name: string;
    value_type: { scalar_type: string; nullable: boolean };
  }>;
};

type CompositionPreviewResponse = {
  ok: boolean;
  plan?: {
    queries: Array<{
      variable_id: string;
      returns: CompositionTableReturn;
      form_bindings: Array<{ relation: string; form_id: string }>;
      schema_fingerprint: string;
    }>;
  };
  diagnostics?: Array<{ code: string }>;
};

type CompositionEvaluationRequest = {
  evaluation_id: string;
  target: {
    kind: "saved_revision";
    composition_id: string;
    revision_id: string;
  };
  requested_queries?: string[];
  page?: { variable_id: string; cursor: string };
};

type CompositionEvaluationTable = {
  kind: "table";
  columns: CompositionTableReturn["columns"];
  rows: unknown[][];
};

type CompositionEvaluationResponse = {
  evaluation_id: string;
  ok: boolean;
  composition_revision?: { entry_id: string; revision_id: string };
  computed_query_ids: string[];
  queries: Array<{ variable_id: string; returns: CompositionTableReturn }>;
  query_results: Record<string, {
    value: CompositionEvaluationTable;
    has_more: boolean;
    next?: string;
  }>;
  [key: string]: unknown;
};

type NarrowSeed = {
  spaceId: string;
  compositionOneColumnId: string;
  compositionOneColumnRevision: string;
  compositionTwoColumnId: string;
  compositionTwoColumnRevision: string;
  singleColumnSqlName: string;
  singleColumnSqlId: string;
  manyColumnSqlId: string;
  alphaValues: string[];
  pagedAlphaValues: string[];
};

const MANY_COLUMN_COUNT = 12;
const COMPOSITION_PAGE_SIZE = 100;

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

function buildInlineSqlCompositionYaml(name: string, sql: string): string {
  const lines = [
    "format: ugoite.composition",
    "format_version: 1",
    `name: ${name}`,
    "kind: dashboard",
    "spec:",
    "  variables:",
    "    - id: narrow_rows",
    "      kind: query",
    "      returns:",
    "        kind: table",
    "        columns: []",
    "      query:",
    "        kind: inline_sql",
    `        sql: ${JSON.stringify(sql)}`,
    "        arguments: []",
    "        form_bindings: []",
    '        schema_fingerprint: ""',
    "  components:",
    "    - id: narrow_table",
    "      kind: table",
    "      variable: narrow_rows",
    "  layout:",
    "    kind: flow",
    "    rows:",
    "      - id: main",
    "        items:",
    "          - kind: component",
    "            component: narrow_table",
    "",
  ];
  return lines.join("\n");
}

async function completeInlineSqlCompositionYaml(
  request: APIRequestContext,
  spaceId: string,
  draftYaml: string,
): Promise<string> {
  const response = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions/preview`),
    { data: { yaml: draftYaml } },
  );
  expect(response.status()).toBe(200);
  const preview = await response.json() as CompositionPreviewResponse;
  expect(preview.ok, JSON.stringify(preview.diagnostics)).toBe(true);
  const query = preview.plan?.queries.find((item) =>
    item.variable_id === "narrow_rows"
  );
  expect(query).toBeTruthy();
  expect(query!.returns.kind).toBe("table");
  expect(query!.returns.columns.length).toBeGreaterThan(0);
  expect(query!.schema_fingerprint).toMatch(/^[0-9a-f]{64}$/);

  let yaml = draftYaml;
  const replaceLine = (line: string, replacement: string) => {
    expect(yaml).toContain(line);
    yaml = yaml.replace(line, replacement);
  };
  replaceLine(
    "      returns:\n        kind: table\n        columns: []",
    `      returns: ${JSON.stringify(query!.returns)}`,
  );
  replaceLine(
    "        form_bindings: []",
    `        form_bindings: ${JSON.stringify(query!.form_bindings)}`,
  );
  replaceLine(
    '        schema_fingerprint: ""',
    `        schema_fingerprint: ${JSON.stringify(query!.schema_fingerprint)}`,
  );
  return yaml;
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
  const pagedAlphaValues = Array.from(
    { length: COMPOSITION_PAGE_SIZE + 1 },
    (_, index) =>
      `Narrow Alpha ${suffix} ${String(index + 1).padStart(3, "0")}`,
  );
  const alphaValues = pagedAlphaValues.slice(0, 3);

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
  const totalOrder = `ORDER BY ${alphaColumn} ASC, _ugoite_id LIMIT 3`;

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

  const oneColumnYaml = await completeInlineSqlCompositionYaml(
    request,
    spaceId,
    buildInlineSqlCompositionYaml(
      `Narrow one column ${suffix}`,
      `SELECT ${alphaColumn} AS label FROM ${relation} ORDER BY ${alphaColumn} ASC, _ugoite_id LIMIT 3`,
    ),
  );
  const oneColumn = await saveComposition(request, spaceId, oneColumnYaml);
  const twoColumnYaml = await completeInlineSqlCompositionYaml(
    request,
    spaceId,
    buildInlineSqlCompositionYaml(
      `Narrow two column ${suffix}`,
      `SELECT ${alphaColumn} AS alpha, ${betaColumn} AS beta FROM ${relation} ORDER BY ${alphaColumn} ASC, _ugoite_id LIMIT ${
        COMPOSITION_PAGE_SIZE + 1
      }`,
    ),
  );
  const twoColumn = await saveComposition(request, spaceId, twoColumnYaml);

  return {
    spaceId,
    compositionOneColumnId: oneColumn.composition_id,
    compositionOneColumnRevision: oneColumn.revision_id,
    compositionTwoColumnId: twoColumn.composition_id,
    compositionTwoColumnRevision: twoColumn.revision_id,
    singleColumnSqlName: `Narrow single column ${suffix}`,
    singleColumnSqlId: singleColumn.id!,
    manyColumnSqlId: manyColumn.id!,
    alphaValues,
    pagedAlphaValues,
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
    // Mitase evidence: REQ-FE-068#criterion.local-horizontal-overflow. Issue #4534.
    test.setTimeout(180_000);
    let firstEvaluationId = "";
    let firstEvaluationResponse: CompositionEvaluationResponse | undefined;
    const pageRequests: CompositionEvaluationRequest[] = [];
    const continuationCursor = "narrow-result-page-2";
    await page.route("**/compositions/evaluate", async (route) => {
      const request = route.request()
        .postDataJSON() as CompositionEvaluationRequest;
      expect(request.target).toEqual({
        kind: "saved_revision",
        composition_id: seed.compositionTwoColumnId,
        revision_id: seed.compositionTwoColumnRevision,
      });

      if (request.page) {
        pageRequests.push(request);
        expect(firstEvaluationResponse).toBeTruthy();
        expect(request.evaluation_id).toBe(firstEvaluationId);
        expect(request.page).toEqual({
          variable_id: "narrow_rows",
          cursor: continuationCursor,
        });
        const previousResult = firstEvaluationResponse!.query_results
          .narrow_rows;
        await route.fulfill({
          json: {
            ...firstEvaluationResponse,
            evaluation_id: request.evaluation_id,
            computed_query_ids: ["narrow_rows"],
            query_results: {
              narrow_rows: {
                value: {
                  ...previousResult.value,
                  rows: [[
                    { kind: "string", value: seed.pagedAlphaValues.at(-1)! },
                    {
                      kind: "string",
                      value: `Narrow Beta ${seed.pagedAlphaValues.at(-1)!}`,
                    },
                  ]],
                },
                has_more: false,
              },
            },
          },
        });
        return;
      }

      firstEvaluationId = request.evaluation_id;
      const response = await route.fetch();
      expect(response.ok()).toBe(true);
      const evaluation = await response.json() as CompositionEvaluationResponse;
      expect(evaluation.ok).toBe(true);
      expect(evaluation.composition_revision).toEqual({
        entry_id: seed.compositionTwoColumnId,
        revision_id: seed.compositionTwoColumnRevision,
      });
      expect(evaluation.query_results.narrow_rows.value.kind).toBe("table");
      firstEvaluationResponse = evaluation;

      // Keep the saved revision's real evaluate response and schema, then
      // expand its page in the browser to exercise the 100-row viewport.
      const existingResult = evaluation.query_results.narrow_rows;
      const firstPageRows = seed.pagedAlphaValues.slice(
        0,
        COMPOSITION_PAGE_SIZE,
      ).map((alpha) => [
        { kind: "string", value: alpha },
        { kind: "string", value: `Narrow Beta ${alpha}` },
      ]);
      await route.fulfill({
        response,
        json: {
          ...evaluation,
          query_results: {
            ...evaluation.query_results,
            narrow_rows: {
              ...existingResult,
              value: { ...existingResult.value, rows: firstPageRows },
              has_more: true,
              next: continuationCursor,
            },
          },
        },
      });
    });

    for (const width of [320, 390]) {
      firstEvaluationId = "";
      firstEvaluationResponse = undefined;
      pageRequests.length = 0;
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
      await expect(table.locator("tbody tr")).toHaveCount(
        COMPOSITION_PAGE_SIZE,
      );
      await expect(table).toContainText(seed.pagedAlphaValues[0]);

      await expectNoPageHorizontalOverflow(page);
      const viewport = page.locator(".result-table-viewport");
      await expect(viewport).toHaveCount(1);
      await expectViewportRectWithinViewport(page, viewport);
      await expectNoGiantFixedMinWidth(table, width);

      const next = page.getByRole("button", { name: "Next", exact: true });
      await expect(next).toBeEnabled();
      await next.click();
      await expect(table.locator("tbody tr")).toHaveCount(1);
      await expect(table).toContainText(seed.pagedAlphaValues.at(-1)!);
      expect(pageRequests).toHaveLength(1);
      expect(pageRequests[0].evaluation_id).toBe(firstEvaluationId);
      expect(pageRequests[0].page).toEqual({
        variable_id: "narrow_rows",
        cursor: continuationCursor,
      });
      await expectNoPageHorizontalOverflow(page);
      await expectViewportRectWithinViewport(page, viewport);
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
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${seed.spaceId}/compositions/new\\?`),
      );
      const bindingPicker = page.getByRole("dialog", { name: "Table" });
      await expect(bindingPicker).toBeVisible();
      await expect(bindingPicker.getByRole("tab", { name: "Saved SQL" }))
        .toHaveAttribute("aria-selected", "true");
      await bindingPicker.getByRole("button", {
        name: seed.singleColumnSqlName,
        exact: true,
      }).click();
      await expect(bindingPicker).toBeHidden();
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
