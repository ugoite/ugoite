import {
  type APIRequestContext,
  type Browser,
  type BrowserContext,
  expect,
  type Page,
  test,
} from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";

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

type ResultColumn = { name: string; type: "float" | "integer" };
type SourceDefinition = {
  id: string;
  savedSql: SavedSqlRevision;
  columns: ResultColumn[];
};
type MetricDefinition = {
  id: string;
  label: string;
  source: string;
  column: string;
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

function compositionYaml(
  name: string,
  sources: SourceDefinition[],
  metrics: MetricDefinition[],
): string {
  const lines = [
    "format: ugoite.composition",
    "format_version: 1",
    `name: ${name}`,
    "kind: dashboard",
    "tags: []",
    "spec:",
    "  sources:",
  ];
  for (const source of sources) {
    lines.push(
      `    - id: ${source.id}`,
      "      kind: saved_sql",
      `      entry_id: \"${source.savedSql.entryId}\"`,
      `      revision_id: \"${source.savedSql.revisionId}\"`,
      "      expected_result:",
    );
    for (const column of source.columns) {
      lines.push(
        `        - name: ${column.name}`,
        `          type: ${column.type}`,
      );
    }
  }
  lines.push("  components:");
  for (const metric of metrics) {
    lines.push(
      `    - id: ${metric.id}`,
      "      kind: metric",
      `      label: ${metric.label}`,
      `      source: ${metric.source}`,
      `      column: ${metric.column}`,
    );
  }
  lines.push(
    "  layout:",
    "    kind: flow",
    "    rows:",
    "      - id: aggregates",
    "        items:",
  );
  for (const metric of metrics.slice(0, 4)) {
    lines.push(
      "          - kind: component",
      `            component: ${metric.id}`,
    );
  }
  lines.push("      - id: scalar_states", "        items:");
  for (const metric of metrics.slice(4)) {
    lines.push(
      "          - kind: component",
      `            component: ${metric.id}`,
    );
  }
  lines.push("");
  return lines.join("\n");
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

  const sources: SourceDefinition[] = [
    {
      id: "aggregate_source",
      savedSql: aggregateSql,
      columns: [
        { name: "row_count", type: "integer" },
        { name: "amount_count", type: "integer" },
        { name: "amount_sum", type: "float" },
        { name: "amount_average", type: "float" },
      ],
    },
    {
      id: "zero_rows_source",
      savedSql: zeroRowsSql,
      columns: [{ name: "value", type: "float" }],
    },
    {
      id: "null_source",
      savedSql: nullSql,
      columns: [{ name: "value", type: "float" }],
    },
    {
      id: "zero_source",
      savedSql: zeroSql,
      columns: [{ name: "value", type: "integer" }],
    },
  ];
  const metrics: MetricDefinition[] = [
    {
      id: "all_rows",
      label: "All rows",
      source: "aggregate_source",
      column: "row_count",
    },
    {
      id: "rows_with_amount",
      label: "Rows with amount",
      source: "aggregate_source",
      column: "amount_count",
    },
    {
      id: "amount_total",
      label: "Amount total",
      source: "aggregate_source",
      column: "amount_sum",
    },
    {
      id: "amount_average",
      label: "Amount average",
      source: "aggregate_source",
      column: "amount_average",
    },
    {
      id: "zero_rows",
      label: "No matching rows",
      source: "zero_rows_source",
      column: "value",
    },
    {
      id: "null_result",
      label: "Empty sum",
      source: "null_source",
      column: "value",
    },
    {
      id: "zero_result",
      label: "Empty count",
      source: "zero_source",
      column: "value",
    },
  ];
  const compositionResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions`),
    {
      headers: { "Idempotency-Key": crypto.randomUUID() },
      data: { yaml: compositionYaml(compositionName, sources, metrics) },
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

function metricSection(page: Page, label: string) {
  return page.locator("section.section").filter({
    has: page.getByRole("heading", { name: label, exact: true }),
  });
}

function metricOutput(page: Page, label: string) {
  return metricSection(page, label).locator("output.compositionMetric");
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
      await expect(metricOutput(page, "All rows")).toHaveText("3", {
        timeout: scalarResultTimeoutMs,
      });
      await expect(metricOutput(page, "Rows with amount")).toHaveText("2", {
        timeout: scalarResultTimeoutMs,
      });
      await expect(metricOutput(page, "Amount total")).toHaveText("30", {
        timeout: scalarResultTimeoutMs,
      });
      await expect(metricOutput(page, "Amount average")).toHaveText("15", {
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
      const zeroRowsSection = metricSection(page, "No matching rows");
      await expect(zeroRowsSection.getByRole("alert")).toContainText(
        await page.locator("html").getAttribute("lang").then((locale) =>
          locale === "ja"
            ? "指標の結果がありません。"
            : "The metric returned no rows."
        ),
        { timeout: scalarResultTimeoutMs },
      );
      await expect(zeroRowsSection.locator("output.compositionMetric"))
        .toHaveCount(0, { timeout: scalarResultTimeoutMs });

      const nullOutput = metricOutput(page, "Empty sum");
      await expect(nullOutput).toHaveText(
        await page.locator("html").getAttribute("lang").then((locale) =>
          locale === "ja" ? "データなし" : "No data"
        ),
        { timeout: scalarResultTimeoutMs },
      );
      await expect(metricOutput(page, "Empty count")).toHaveText("0", {
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
