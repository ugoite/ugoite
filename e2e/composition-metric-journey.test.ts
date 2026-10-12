import {
  type APIRequestContext,
  type Browser,
  type BrowserContext,
  expect,
  test,
} from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";

const METRIC_SCALAR = 40;
const SECOND_AMOUNT = 2;
const MULTIPLE_ROWS_DIAGNOSTIC = "The metric returned more than one row.";

type SeedState = {
  spaceId: string;
  formName: string;
  compositionId: string;
  revisionId: string;
  compositionName: string;
};

type MetricUpdateSeed = {
  spaceId: string;
  formName: string;
  compositionId: string;
  revisionId: string;
  compositionName: string;
};

type FormFieldSummary = {
  id?: number;
  query_capability?: { field?: { field_id?: number } };
};

type FormSummary = {
  id?: string;
  name?: string;
  sql_relation?: string;
  fields?: Record<string, FormFieldSummary & { sql_column?: string }>;
};

function fieldId(field: FormFieldSummary | undefined): number {
  const resolved = field?.query_capability?.field?.field_id ?? field?.id;
  expect(resolved).toBeTruthy();
  return resolved!;
}

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

async function createSpace(request: APIRequestContext): Promise<string> {
  const suffix = `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
  const response = await request.post(getBackendUrl("/spaces"), {
    data: {
      slug: `composition-metric-${suffix}`,
      name: `Composition Metric ${suffix}`,
    },
  });
  expect([200, 201]).toContain(response.status());
  const result = await response.json() as { space_uid?: string };
  expect(result.space_uid).toBeTruthy();
  return result.space_uid!;
}

async function seedMetricComposition(
  request: APIRequestContext,
): Promise<SeedState> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `MetricScalar${suffix}`;
  const compositionName = `Metric scalar ${suffix}`;

  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Date\n\n## Merchant\n\n## Amount\n`,
        fields: {
          Date: { type: "date", required: true },
          Merchant: { type: "string", required: true },
          Amount: { type: "double", required: true },
        },
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
  const dateColumn = form?.fields?.Date?.sql_column;
  const merchantColumn = form?.fields?.Merchant?.sql_column;
  const amountColumn = form?.fields?.Amount?.sql_column;
  expect(relation).toBeTruthy();
  expect(dateColumn).toBeTruthy();
  expect(merchantColumn).toBeTruthy();
  expect(amountColumn).toBeTruthy();

  const entryResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/entries`),
    {
      data: {
        form: formName,
        fields: {
          Date: "2026-04-10",
          Merchant: "Metric-One",
          Amount: METRIC_SCALAR,
        },
      },
    },
  );
  expect(entryResponse.status()).toBe(201);

  const sqlResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name: `Metric scalar query ${suffix}`,
        kind: "user-query",
        sql: `SELECT ${sqlIdentifier(amountColumn!)} AS total, ${
          sqlIdentifier(merchantColumn!)
        } AS merchant FROM ${sqlIdentifier(relation!)} ORDER BY ${
          sqlIdentifier(merchantColumn!)
        } ASC`,
        variables: [],
      },
    },
  );
  expect([200, 201]).toContain(sqlResponse.status());
  const savedSql = await sqlResponse.json() as {
    id?: string;
    revision_id?: string;
  };
  expect(savedSql.id).toBeTruthy();
  expect(savedSql.revision_id).toBeTruthy();

  const yaml = [
    "format: ugoite.composition",
    "format_version: 1",
    `name: ${compositionName}`,
    "kind: dashboard",
    "tags: []",
    "spec:",
    "  sources:",
    "    - id: metric_rows",
    "      kind: saved_sql",
    `      entry_id: "${savedSql.id}"`,
    `      revision_id: "${savedSql.revision_id}"`,
    "      expected_result:",
    "        - name: total",
    "          type: float",
    "        - name: merchant",
    "          type: string",
    "  components:",
    "    - id: total",
    "      kind: metric",
    "      source: metric_rows",
    "      column: total",
    "    - id: rows",
    "      kind: table",
    "      source: metric_rows",
    "  layout:",
    "    kind: flow",
    "    rows:",
    "      - id: main",
    "        items:",
    "          - kind: component",
    "            component: total",
    "          - kind: component",
    "            component: rows",
    "",
  ].join("\n");
  const saveResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions`),
    {
      headers: { "Idempotency-Key": crypto.randomUUID() },
      data: { yaml },
    },
  );
  expect([200, 201]).toContain(saveResponse.status());
  const saved = await saveResponse.json() as {
    composition_id?: string;
    revision_id?: string;
  };
  expect(saved.composition_id).toBeTruthy();
  expect(saved.revision_id).toBeTruthy();

  return {
    spaceId,
    formName,
    compositionId: saved.composition_id!,
    revisionId: saved.revision_id!,
    compositionName,
  };
}

async function seedMetricUpdateComposition(
  request: APIRequestContext,
): Promise<MetricUpdateSeed> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `MetricUpdate${suffix}`;
  const compositionName = `Metric update ${suffix}`;

  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Merchant\n`,
        fields: {
          Merchant: { type: "string", required: true },
        },
      },
    },
  );
  expect([200, 201]).toContain(formResponse.status());

  const formsResponse = await request.get(
    getBackendUrl(`/spaces/${spaceId}/forms`),
  );
  expect(formsResponse.ok()).toBe(true);
  const forms = await formsResponse.json() as FormSummary[];
  const form = forms.find((candidate) => candidate.name === formName);
  expect(form?.id).toBeTruthy();
  expect(form?.sql_relation).toBeTruthy();
  const merchantField = form?.fields?.Merchant;
  expect(merchantField?.sql_column).toBeTruthy();
  const merchantFieldId = fieldId(merchantField);

  for (const merchant of ["Metric-One", "Metric-Two"] as const) {
    const entryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      {
        data: {
          form: formName,
          fields: { Merchant: merchant },
        },
      },
    );
    expect(entryResponse.status()).toBe(201);
  }

  const sqlResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name: `Metric update count ${suffix}`,
        kind: "user-query",
        sql: `SELECT COUNT(*) AS total FROM ${
          sqlIdentifier(form!.sql_relation!)
        }`,
        variables: [],
      },
    },
  );
  expect([200, 201]).toContain(sqlResponse.status());
  const savedSql = await sqlResponse.json() as {
    id?: string;
    revision_id?: string;
  };
  expect(savedSql.id).toBeTruthy();
  expect(savedSql.revision_id).toBeTruthy();

  const yaml = [
    "format: ugoite.composition",
    "format_version: 1",
    `name: ${compositionName}`,
    "kind: dashboard",
    "tags: []",
    "spec:",
    "  sources:",
    "    - id: metric_count",
    "      kind: saved_sql",
    `      entry_id: "${savedSql.id}"`,
    `      revision_id: "${savedSql.revision_id}"`,
    "      expected_result:",
    "        - name: total",
    "          type: integer",
    "    - id: entry_rows",
    "      kind: entry_query",
    `      form_id: "${form!.id}"`,
    "      field_schema:",
    "        - field_id: " + merchantFieldId,
    "          field_type: string",
    "      query:",
    "        page_limit: 1",
    "        sort:",
    `          - field_id: ${merchantFieldId}`,
    "            direction: asc",
    "        projection:",
    "          kind: fields",
    `          fields: [${merchantFieldId}]`,
    "  components:",
    "    - id: total",
    "      kind: metric",
    "      source: metric_count",
    "      column: total",
    "    - id: entries",
    "      kind: table",
    "      source: entry_rows",
    "  layout:",
    "    kind: flow",
    "    rows:",
    "      - id: main",
    "        items:",
    "          - kind: component",
    "            component: total",
    "          - kind: component",
    "            component: entries",
    "",
  ].join("\n");
  const saveResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions`),
    {
      headers: { "Idempotency-Key": crypto.randomUUID() },
      data: { yaml },
    },
  );
  expect([200, 201]).toContain(saveResponse.status());
  const saved = await saveResponse.json() as {
    composition_id?: string;
    revision_id?: string;
  };
  expect(saved.composition_id).toBeTruthy();
  expect(saved.revision_id).toBeTruthy();

  return {
    spaceId,
    formName,
    compositionId: saved.composition_id!,
    revisionId: saved.revision_id!,
    compositionName,
  };
}

async function openOwnerContext(
  browser: Browser,
  storageState: Awaited<ReturnType<BrowserContext["storageState"]>>,
) {
  return await browser.newContext({ storageState });
}

test.describe("Composition Metric Journey", () => {
  let seed: SeedState;

  test.beforeAll(async ({ request }) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    seed = await seedMetricComposition(request);
  });

  test("Browser reopens a Saved SQL metric at its exact revision and reports multiple matching rows", async ({ browser, context, request }) => {
    test.setTimeout(120_000);
    const storageState = await context.storageState();
    const freshContext = await openOwnerContext(browser, storageState);
    try {
      const page = await freshContext.newPage();
      const path =
        `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/${seed.revisionId}`;
      await page.goto(getFrontendUrl(`/spaces/${seed.spaceId}/dashboard`), {
        waitUntil: "domcontentloaded",
      });
      const link = page.getByRole("link", {
        name: new RegExp(`^${seed.compositionName} Dashboard`),
      });
      await expect(link).toHaveAttribute("href", path);
      await link.click();
      await expect(page).toHaveURL(new RegExp(`${path}$`));
      await expect(
        page.getByRole("heading", { level: 1, name: seed.compositionName }),
      ).toBeVisible();

      const metric = page.locator("output.compositionMetric");
      await expect(metric).toHaveText(String(METRIC_SCALAR));
      const table = page.getByRole("table");
      await expect(table.locator("tbody tr")).toHaveCount(1);
      await expect(table).toContainText("Metric-One");

      const secondEntryResponse = await request.post(
        getBackendUrl(`/spaces/${seed.spaceId}/entries`),
        {
          data: {
            form: seed.formName,
            fields: {
              Date: "2026-04-11",
              Merchant: "Metric-Two",
              Amount: SECOND_AMOUNT,
            },
          },
        },
      );
      expect(secondEntryResponse.status()).toBe(201);

      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(page).toHaveURL(new RegExp(`${path}$`));
      await expect(page.getByRole("alert")).toContainText(
        MULTIPLE_ROWS_DIAGNOSTIC,
      );
      await expect(page.getByRole("button", { name: "Edit", exact: true }))
        .toBeVisible();
      await expect(page.locator("output.compositionMetric")).toHaveCount(0);
      await expect(table.locator("tbody tr")).toHaveCount(2);
    } finally {
      await freshContext.close();
    }
  });

  test("Browser reevaluates a Saved SQL metric after reload when an Entry is added independently of Table pagination", async ({ browser, context, request }) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    const updateSeed = await seedMetricUpdateComposition(request);
    const storageState = await context.storageState();
    const freshContext = await openOwnerContext(browser, storageState);
    try {
      const page = await freshContext.newPage();
      const path =
        `/spaces/${updateSeed.spaceId}/compositions/${updateSeed.compositionId}/${updateSeed.revisionId}`;
      await page.goto(
        getFrontendUrl(`/spaces/${updateSeed.spaceId}/dashboard`),
        { waitUntil: "domcontentloaded" },
      );
      const link = page.getByRole("link", {
        name: new RegExp(`^${updateSeed.compositionName} Dashboard`),
      });
      await expect(link).toHaveAttribute("href", path);
      await link.click();
      await expect(page).toHaveURL(new RegExp(`${path}$`));
      await expect(
        page.getByRole("heading", {
          level: 1,
          name: updateSeed.compositionName,
        }),
      ).toBeVisible();

      const metric = page.locator("output.compositionMetric");
      const table = page.getByRole("table");
      await expect(metric).toHaveText("2");
      await expect(table.locator("tbody tr")).toHaveCount(1);
      await expect(table).toContainText("Metric-One");

      await page.getByRole("button", { name: "Next", exact: true }).click();
      await expect(table.locator("tbody tr")).toHaveCount(1);
      await expect(table).toContainText("Metric-Two");
      await expect(metric).toHaveText("2");

      const thirdEntryResponse = await request.post(
        getBackendUrl(`/spaces/${updateSeed.spaceId}/entries`),
        {
          data: {
            form: updateSeed.formName,
            fields: { Merchant: "Metric-Three" },
          },
        },
      );
      expect(thirdEntryResponse.status()).toBe(201);

      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(page).toHaveURL(new RegExp(`${path}$`));
      await expect(metric).toHaveText("3");
      await expect(table.locator("tbody tr")).toHaveCount(1);
      await expect(table).toContainText("Metric-One");
      await page.getByRole("button", { name: "Next", exact: true }).click();
      await expect(table).toContainText("Metric-Three");
      await expect(metric).toHaveText("3");
    } finally {
      await freshContext.close();
    }
  });
});
