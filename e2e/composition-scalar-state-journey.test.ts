import {
  type APIRequestContext,
  type Browser,
  type BrowserContext,
  expect,
  type Page,
  test,
} from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";
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
  const suffix = `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
  const response = await request.post(getBackendUrl("/spaces"), {
    data: {
      slug: `composition-scalar-state-${suffix}`,
      name: `Composition scalar state ${suffix}`,
    },
  });
  expect([200, 201]).toContain(response.status());
  const result = await response.json() as { space_uid?: string };
  expect(result.space_uid).toBeTruthy();
  return result.space_uid!;
}

async function createSavedSql(
  request: APIRequestContext,
  spaceId: string,
  name: string,
  sql: string,
): Promise<SavedSqlRevision> {
  const response = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    { data: { name, kind: "user-query", sql, variables: [] } },
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
): Promise<SeedState> {
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
  const zeroRowsSql = await createSavedSql(
    request,
    spaceId,
    `Scalar empty rows ${suffix}`,
    `SELECT ${amount} AS value FROM ${table} WHERE ${amount} < 0`,
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
      id: "zero_rows",
      savedSql: zeroRowsSql,
      column: "value",
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
  await page.goto(getFrontendUrl(`/spaces/${seed.spaceId}/dashboard`), {
    waitUntil: "domcontentloaded",
  });
  const link = page.getByRole("link", {
    name: new RegExp(`^${seed.compositionName} Dashboard`),
  });
  await expect(link).toHaveAttribute("href", path);
  await link.click();
  await page.waitForURL((url) => url.pathname === path);
  await expect(
    page.getByRole("heading", { level: 1, name: seed.compositionName }),
  ).toBeVisible();
}

function metricSection(page: Page, componentIndex: number) {
  return page.locator("section.section").nth(componentIndex);
}

function metricOutput(page: Page, componentIndex: number) {
  return metricSection(page, componentIndex).locator(
    ".compositionGraphMetric strong",
  );
}

const scalarResultTimeoutMs = 30_000;

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
  let seed: SeedState;

  test.beforeAll(async ({ request }) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    seed = await seedScalarStateComposition(request);
  });

  test("sql_aggregates_preserve_null_and_empty_semantics", async ({ browser, context }) => {
    test.setTimeout(120_000);
    const { freshContext, page } = await openOwnerPage(browser, context, seed);
    try {
      await expect(metricOutput(page, 0)).toHaveText("3", {
        timeout: scalarResultTimeoutMs,
      });
      await expect(metricOutput(page, 1)).toHaveText("2", {
        timeout: scalarResultTimeoutMs,
      });
      await expect(metricOutput(page, 2)).toHaveText("30", {
        timeout: scalarResultTimeoutMs,
      });
      await expect(metricOutput(page, 3)).toHaveText("15", {
        timeout: scalarResultTimeoutMs,
      });
    } finally {
      await freshContext.close();
    }
  });

  test("scalar_result_distinguishes_zero_rows_null_and_zero", async ({ browser, context }) => {
    test.setTimeout(120_000);
    const { freshContext, page } = await openOwnerPage(browser, context, seed);
    try {
      const zeroRowsSection = metricSection(page, 4);
      await expect(zeroRowsSection.getByRole("alert")).toContainText(
        await page.locator("html").getAttribute("lang").then((locale) =>
          locale === "ja"
            ? "指標の結果がありません。"
            : "The metric returned no rows."
        ),
        { timeout: scalarResultTimeoutMs },
      );
      await expect(zeroRowsSection.locator(".compositionGraphMetric strong"))
        .toHaveCount(0, { timeout: scalarResultTimeoutMs });

      const nullOutput = metricOutput(page, 5);
      await expect(nullOutput).toHaveText(
        await page.locator("html").getAttribute("lang").then((locale) =>
          locale === "ja" ? "データなし" : "No data"
        ),
        { timeout: scalarResultTimeoutMs },
      );
      await expect(metricOutput(page, 6)).toHaveText("0", {
        timeout: scalarResultTimeoutMs,
      });
      await expect(nullOutput).not.toHaveText("0", {
        timeout: scalarResultTimeoutMs,
      });
    } finally {
      await freshContext.close();
    }
  });
});
