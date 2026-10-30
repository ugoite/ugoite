import {
  type APIRequestContext,
  type Browser,
  type BrowserContext,
  expect,
  type Page,
  test,
} from "@playwright/test";
import {
  getBackendUrl,
  getDefaultSpaceId,
  getFrontendUrl,
  waitForServers,
} from "./lib/client.ts";
import {
  completeCompositionGraphYaml,
  type CompositionGraphSeed,
} from "./lib/composition-graph.ts";

type SavedSqlRevision = {
  entryId: string;
  revisionId: string;
};

type SeedState = {
  spaceId: string;
  compositionId: string;
  compositionRevisionId: string;
  compositionName: string;
};

type ScalarQueryDefinition = {
  id: string;
  savedSql: SavedSqlRevision;
  column: string;
  scalarType: "integer" | "float";
  nullable: boolean;
};

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

async function createSpace(request: APIRequestContext): Promise<string> {
  return await getDefaultSpaceId(request);
}

async function createSavedSql(
  request: APIRequestContext,
  spaceId: string,
  name: string,
  sql: string,
  variables: Array<{ name: string; type: string; description: string }> = [],
): Promise<SavedSqlRevision> {
  const response = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    { data: { name, kind: "user-query", sql, variables } },
  );
  expect([200, 201]).toContain(response.status());
  const savedSql = await response.json() as {
    id?: string;
    revision_id?: string;
  };
  expect(savedSql.id).toBeTruthy();
  expect(savedSql.revision_id).toBeTruthy();
  return { entryId: savedSql.id!, revisionId: savedSql.revision_id! };
}

function compositionGraph(
  name: string,
  queries: ScalarQueryDefinition[],
): CompositionGraphSeed {
  const queryVariable = (query: ScalarQueryDefinition) => ({
    id: query.id,
    kind: "query" as const,
    returns: {
      kind: "scalar",
      value_type: {
        scalar_type: query.scalarType,
        nullable: query.nullable,
      },
    },
    query: {
      kind: "saved_sql",
      entry_id: query.savedSql.entryId,
      revision_id: query.savedSql.revisionId,
      column: query.column,
      arguments: [],
      form_bindings: [],
      schema_fingerprint: "",
    },
  });
  return {
    name,
    variables: queries.map(queryVariable),
    components: queries.map((query) => ({
      id: query.id,
      kind: "metric",
      variable: query.id,
    })),
    rows: [
      {
        id: "aggregates",
        items: queries.slice(0, 4).map((query) => ({
          kind: "component" as const,
          component: query.id,
        })),
      },
      {
        id: "scalar_states",
        items: queries.slice(4).map((query) => ({
          kind: "component" as const,
          component: query.id,
        })),
      },
    ],
  };
}

async function seedScalarStateComposition(
  request: APIRequestContext,
): Promise<{ aggregates: SeedState; emptyScalar: SeedState }> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `ScalarState${suffix}`;
  const compositionName = `Reactive scalar states ${suffix}`;
  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Amount\n`,
        fields: { Amount: { type: "double", required: false } },
      },
    },
  );
  expect([200, 201]).toContain(formResponse.status());

  const formsResponse = await request.get(
    getBackendUrl(`/spaces/${spaceId}/forms`),
  );
  expect(formsResponse.ok()).toBe(true);
  const forms = await formsResponse.json() as Array<{
    name?: string;
    sql_relation?: string;
    fields?: Record<string, { sql_column?: string }>;
  }>;
  const form = forms.find((candidate) => candidate.name === formName);
  const relation = form?.sql_relation;
  const amountColumn = form?.fields?.Amount?.sql_column;
  expect(relation).toBeTruthy();
  expect(amountColumn).toBeTruthy();

  for (const fields of [{ Amount: 10 }, { Amount: 20 }, {}]) {
    const entryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      { data: { form: formName, fields } },
    );
    expect(entryResponse.status()).toBe(201);
  }

  const amount = sqlIdentifier(amountColumn!);
  const table = sqlIdentifier(relation!);
  const aggregateSql = await createSavedSql(
    request,
    spaceId,
    `Scalar aggregate ${suffix}`,
    `SELECT COUNT(*) AS row_count, COUNT(${amount}) AS amount_count, ` +
      `SUM(${amount}) AS amount_sum, AVG(${amount}) AS amount_average ` +
      `FROM ${table}`,
  );
  const nullSql = await createSavedSql(
    request,
    spaceId,
    `Scalar null ${suffix}`,
    `SELECT SUM(${amount}) AS value FROM ${table} WHERE ${amount} < 0`,
  );
  const zeroSql = await createSavedSql(
    request,
    spaceId,
    `Scalar zero ${suffix}`,
    `SELECT COUNT(*) AS value FROM ${table} WHERE ${amount} < 0`,
  );

  const queries: ScalarQueryDefinition[] = [
    {
      id: "all_rows",
      savedSql: aggregateSql,
      column: "row_count",
      scalarType: "integer",
      nullable: false,
    },
    {
      id: "rows_with_amount",
      savedSql: aggregateSql,
      column: "amount_count",
      scalarType: "integer",
      nullable: false,
    },
    {
      id: "amount_total",
      savedSql: aggregateSql,
      column: "amount_sum",
      scalarType: "float",
      nullable: true,
    },
    {
      id: "amount_average",
      savedSql: aggregateSql,
      column: "amount_average",
      scalarType: "float",
      nullable: true,
    },
    {
      id: "null_result",
      savedSql: nullSql,
      column: "value",
      scalarType: "float",
      nullable: true,
    },
    {
      id: "zero_result",
      savedSql: zeroSql,
      column: "value",
      scalarType: "integer",
      nullable: false,
    },
  ];
  const yaml = await completeCompositionGraphYaml(
    request,
    spaceId,
    compositionGraph(compositionName, queries),
  );
  const aggregates = await saveCompositionSeed(
    request,
    spaceId,
    yaml,
    compositionName,
  );

  const zeroRowsSql = await createSavedSql(
    request,
    spaceId,
    `Scalar empty rows ${suffix}`,
    `SELECT ${amount} AS value FROM ${table} WHERE ${amount} = $minimum`,
    [{
      name: "minimum",
      type: "integer",
      description: "Amount that must match the scalar result.",
    }],
  );
  const multipleRowsSql = await createSavedSql(
    request,
    spaceId,
    `Scalar multiple rows ${suffix}`,
    `SELECT ${amount} AS value FROM ${table} WHERE ${amount} <= $maximum`,
    [{
      name: "maximum",
      type: "integer",
      description: "Maximum amount included in the scalar result.",
    }],
  );
  const zeroRowsYaml = await completeCompositionGraphYaml(request, spaceId, {
    name: `Empty scalar after Input ${suffix}`,
    variables: [
      {
        id: "minimum",
        kind: "input",
        value_type: { scalar_type: "integer", nullable: false },
        required: true,
        default: { kind: "integer", value: 10 },
      },
      {
        id: "zero_rows",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "float", nullable: true },
        },
        query: {
          kind: "saved_sql",
          entry_id: zeroRowsSql.entryId,
          revision_id: zeroRowsSql.revisionId,
          column: "value",
          arguments: [{ name: "minimum", variable: "minimum" }],
          form_bindings: [],
          schema_fingerprint: "",
        },
      },
      {
        id: "maximum",
        kind: "input",
        value_type: { scalar_type: "integer", nullable: false },
        required: true,
        default: { kind: "integer", value: 10 },
      },
      {
        id: "multiple_rows",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "float", nullable: true },
        },
        query: {
          kind: "saved_sql",
          entry_id: multipleRowsSql.entryId,
          revision_id: multipleRowsSql.revisionId,
          column: "value",
          arguments: [{ name: "maximum", variable: "maximum" }],
          form_bindings: [],
          schema_fingerprint: "",
        },
      },
    ],
    components: [
      { id: "minimum_input", kind: "input", variable: "minimum" },
      { id: "maximum_input", kind: "input", variable: "maximum" },
      { id: "zero_rows", kind: "metric", variable: "zero_rows" },
      { id: "multiple_rows", kind: "metric", variable: "multiple_rows" },
    ],
    rows: [
      {
        id: "inputs",
        items: ["minimum_input", "maximum_input"].map((component) => ({
          kind: "component" as const,
          component,
        })),
      },
      {
        id: "scalar_states",
        items: ["zero_rows", "multiple_rows"].map((component) => ({
          kind: "component" as const,
          component,
        })),
      },
    ],
  });
  const emptyScalar = await saveCompositionSeed(
    request,
    spaceId,
    zeroRowsYaml,
    `Empty scalar after Input ${suffix}`,
  );
  return { aggregates, emptyScalar };
}

async function saveCompositionSeed(
  request: APIRequestContext,
  spaceId: string,
  yaml: string,
  compositionName: string,
): Promise<SeedState> {
  const compositionResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions`),
    {
      headers: { "Idempotency-Key": crypto.randomUUID() },
      data: { yaml },
    },
  );
  expect([200, 201]).toContain(compositionResponse.status());
  const savedComposition = await compositionResponse.json() as {
    composition_id?: string;
    revision_id?: string;
  };
  expect(savedComposition.composition_id).toBeTruthy();
  expect(savedComposition.revision_id).toBeTruthy();
  return {
    spaceId,
    compositionId: savedComposition.composition_id!,
    compositionRevisionId: savedComposition.revision_id!,
    compositionName,
  };
}

async function openSavedComposition(
  page: Page,
  seed: SeedState,
): Promise<void> {
  const path =
    `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/${seed.compositionRevisionId}`;
  await page.goto(getFrontendUrl(`/spaces/${seed.spaceId}/compositions`), {
    waitUntil: "domcontentloaded",
  });
  const link = page.locator(`a[href="${path}"]`);
  await expect(link).toBeVisible();
  await link.click();
  await page.waitForURL((url) => url.pathname === path);
  await expect(
    page.getByRole("heading", { level: 1, name: seed.compositionName }),
  ).toBeVisible();
}

function metricSection(page: Page, variableId: string) {
  return page.getByRole("region", { name: variableId, exact: true });
}

function metricOutput(page: Page, variableId: string) {
  return metricSection(page, variableId).locator("strong");
}

const scalarResultTimeoutMs = 30_000;

test("Composition preserves signed 64-bit SQL results in Table and Metric after reopening", async ({ page, request }) => {
  test.setTimeout(120_000);
  await waitForServers(request);
  const spaceId = await createSpace(request);
  const name = `Exact integer ${crypto.randomUUID().slice(0, 8)}`;
  const sql = "SELECT CAST('9223372036854775807' AS BIGINT) AS maximum, " +
    "CAST('-9223372036854775808' AS BIGINT) AS minimum";
  const inline = {
    kind: "inline_sql",
    sql,
    arguments: [],
    form_bindings: [],
    schema_fingerprint: "",
  };
  const yaml = await completeCompositionGraphYaml(request, spaceId, {
    name,
    variables: [
      {
        id: "bounds",
        kind: "query",
        returns: { kind: "table", columns: [] },
        query: inline,
      },
      {
        id: "maximum",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: {
          ...inline,
          sql: "SELECT CAST('9223372036854775807' AS BIGINT) AS maximum",
        },
      },
      {
        id: "minimum",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: {
          ...inline,
          sql: "SELECT CAST('-9223372036854775808' AS BIGINT) AS minimum",
        },
      },
    ],
    components: [
      { id: "table", kind: "table", variable: "bounds" },
      { id: "max_metric", kind: "metric", variable: "maximum" },
      { id: "min_metric", kind: "metric", variable: "minimum" },
    ],
    rows: [{
      id: "values",
      items: ["table", "max_metric", "min_metric"].map((component) => ({
        kind: "component" as const,
        component,
      })),
    }],
  });
  const seed = await saveCompositionSeed(request, spaceId, yaml, name);
  await openSavedComposition(page, seed);
  for (let visit = 0; visit < 2; visit++) {
    const table = page.getByRole("table", { name: "bounds results" });
    await expect(table).toContainText("9223372036854775807", {
      timeout: scalarResultTimeoutMs,
    });
    await expect(table).toContainText("-9223372036854775808", {
      timeout: scalarResultTimeoutMs,
    });
    await expect(metricOutput(page, "maximum")).toHaveText(
      "9223372036854775807",
      { timeout: scalarResultTimeoutMs },
    );
    await expect(metricOutput(page, "minimum")).toHaveText(
      "-9223372036854775808",
      { timeout: scalarResultTimeoutMs },
    );
    if (visit === 0) await page.reload({ waitUntil: "domcontentloaded" });
  }
});

async function openOwnerPage(
  browser: Browser,
  context: BrowserContext,
  seed: SeedState,
) {
  const freshContext = await browser.newContext({
    storageState: await context.storageState(),
  });
  const page = await freshContext.newPage();
  await openSavedComposition(page, seed);
  return { freshContext, page };
}

test.describe("Composition scalar state journey", () => {
  let seeds: { aggregates: SeedState; emptyScalar: SeedState };

  test.beforeAll(async ({ request }) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    seeds = await seedScalarStateComposition(request);
  });

  test("sql_aggregates_preserve_null_and_empty_semantics", async ({ browser, context }) => {
    test.setTimeout(120_000);
    const { freshContext, page } = await openOwnerPage(
      browser,
      context,
      seeds.aggregates,
    );
    try {
      await expect(metricOutput(page, "all_rows")).toHaveText("3", {
        timeout: scalarResultTimeoutMs,
      });
      await expect(metricOutput(page, "rows_with_amount")).toHaveText("2", {
        timeout: scalarResultTimeoutMs,
      });
      await expect(metricOutput(page, "amount_total")).toHaveText("30", {
        timeout: scalarResultTimeoutMs,
      });
      await expect(metricOutput(page, "amount_average")).toHaveText("15", {
        timeout: scalarResultTimeoutMs,
      });
    } finally {
      await freshContext.close();
    }
  });

  test("scalar_result_distinguishes_zero_rows_null_and_zero", async ({ browser, context }) => {
    test.setTimeout(120_000);
    const { freshContext: aggregateContext, page: aggregatePage } =
      await openOwnerPage(browser, context, seeds.aggregates);
    try {
      const nullOutput = metricOutput(aggregatePage, "null_result");
      await expect(nullOutput).toHaveText(
        await aggregatePage.locator("html").getAttribute("lang").then((
          locale,
        ) => locale === "ja" ? "データなし" : "No data"),
        { timeout: scalarResultTimeoutMs },
      );
      await expect(metricOutput(aggregatePage, "zero_result")).toHaveText("0", {
        timeout: scalarResultTimeoutMs,
      });
      await expect(nullOutput).not.toHaveText("0", {
        timeout: scalarResultTimeoutMs,
      });
    } finally {
      await aggregateContext.close();
    }

    const { freshContext, page } = await openOwnerPage(
      browser,
      context,
      seeds.emptyScalar,
    );
    try {
      const minimumInput = page.getByRole("textbox", {
        name: "minimum",
        exact: true,
      });
      await expect(minimumInput).toHaveValue("10");
      await expect(metricOutput(page, "zero_rows")).toHaveText("10", {
        timeout: scalarResultTimeoutMs,
      });
      await minimumInput.fill("-1");
      const zeroRowsSection = page.getByRole("region", {
        name: "scalar_states",
        exact: true,
      });
      await expect(zeroRowsSection.getByRole("alert")).toContainText(
        await page.locator("html").getAttribute("lang").then((locale) =>
          locale === "ja"
            ? "指標の結果がありません。"
            : "The metric returned no rows."
        ),
        { timeout: scalarResultTimeoutMs },
      );
      await expect(page.getByRole("region", {
        name: "zero_rows",
        exact: true,
      })).toHaveCount(0, { timeout: scalarResultTimeoutMs });
      await minimumInput.fill("10");
      await expect(metricOutput(page, "zero_rows")).toHaveText("10", {
        timeout: scalarResultTimeoutMs,
      });

      const maximumInput = page.getByRole("textbox", {
        name: "maximum",
        exact: true,
      });
      await expect(maximumInput).toHaveValue("10");
      await expect(metricOutput(page, "multiple_rows")).toHaveText("10", {
        timeout: scalarResultTimeoutMs,
      });
      const failedEvaluation = page.waitForResponse((response) => {
        if (
          response.request().method() !== "POST" ||
          !new URL(response.url()).pathname.endsWith("/compositions/evaluate")
        ) return false;
        return response.request().postDataJSON().variables?.maximum === 20;
      });
      await maximumInput.fill("20");
      const result = await (await failedEvaluation).json();
      expect(result.ok).toBe(false);
      expect(result.query_diagnostics).toEqual(expect.arrayContaining([
        expect.objectContaining({
          variable_id: "multiple_rows",
          result_diagnostic: expect.objectContaining({
            result_diagnostic: expect.objectContaining({
              code: "scalar_multiple_rows",
            }),
          }),
        }),
      ]));
      const multipleRowsSection = page.getByRole("region", {
        name: "scalar_states",
        exact: true,
      });
      await expect(multipleRowsSection.getByRole("alert")).toContainText(
        await page.locator("html").getAttribute("lang").then((locale) =>
          locale === "ja"
            ? "指標の結果が複数行です。"
            : "The metric returned more than one row."
        ),
        { timeout: scalarResultTimeoutMs },
      );
      await expect(page.getByRole("region", {
        name: "multiple_rows",
        exact: true,
      })).toHaveCount(0, { timeout: scalarResultTimeoutMs });
      await maximumInput.fill("10");
      await expect(metricOutput(page, "multiple_rows")).toHaveText("10", {
        timeout: scalarResultTimeoutMs,
      });
    } finally {
      await freshContext.close();
    }
  });
});
