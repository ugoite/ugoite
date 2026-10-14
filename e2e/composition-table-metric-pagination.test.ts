import { type APIRequestContext, expect, test } from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";

type SavedSqlRevision = { entryId: string; revisionId: string };

type SeedState = {
  spaceId: string;
  compositionId: string;
  revisionId: string;
  compositionName: string;
};

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

async function createSpace(request: APIRequestContext): Promise<string> {
  const suffix = `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
  const response = await request.post(getBackendUrl("/spaces"), {
    data: {
      slug: `composition-page-metric-${suffix}`,
      name: `Composition page metric ${suffix}`,
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
    {
      data: {
        name,
        kind: "user-query",
        sql,
        variables: [{
          type: "float",
          name: "minimum",
          description: "Minimum amount to include.",
        }],
      },
    },
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

async function seedComposition(
  request: APIRequestContext,
): Promise<SeedState> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `PageMetric${suffix}`;
  const compositionName = `Page metric ${suffix}`;

  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Amount\n\n## Merchant\n`,
        fields: {
          Amount: { type: "double", required: true },
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
  const forms = await formsResponse.json() as Array<{
    id?: string;
    name?: string;
    sql_relation?: string;
    fields?: Record<string, {
      id?: number;
      sql_column?: string;
      query_capability?: { field?: { field_id?: number } };
    }>;
  }>;
  const form = forms.find((candidate) => candidate.name === formName);
  const relation = form?.sql_relation;
  const amountColumn = form?.fields?.Amount?.sql_column;
  const amountFieldId = form?.fields?.Amount?.query_capability?.field
    ?.field_id ?? form?.fields?.Amount?.id;
  const merchantFieldId = form?.fields?.Merchant?.query_capability?.field
    ?.field_id ?? form?.fields?.Merchant?.id;
  expect(form?.id).toBeTruthy();
  expect(relation).toBeTruthy();
  expect(amountColumn).toBeTruthy();
  expect(amountFieldId).toBeTruthy();
  expect(merchantFieldId).toBeTruthy();

  // EntryQuery's small page limit creates multiple pages without a large seed.
  for (const amount of [0, 1, 2]) {
    const entryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      {
        data: {
          form: formName,
          fields: {
            Amount: amount,
            Merchant: `Pager-${String(amount).padStart(3, "0")}`,
          },
        },
      },
    );
    expect(entryResponse.status()).toBe(201);
  }

  const metricSql = await createSavedSql(
    request,
    spaceId,
    `Matching count ${suffix}`,
    `SELECT COUNT(*) AS matching_count FROM ${sqlIdentifier(relation!)} WHERE ${
      sqlIdentifier(amountColumn!)
    } >= $minimum`,
  );

  const yaml = [
    "format: ugoite.composition",
    "format_version: 1",
    `name: ${compositionName}`,
    "kind: dashboard",
    "tags: []",
    "spec:",
    "  variables:",
    "    - id: minimum_amount",
    "      type: float",
    "      required: true",
    "      default: 1",
    "  sources:",
    "    - id: paged_rows",
    "      kind: entry_query",
    `      form_id: "${form!.id}"`,
    "      field_schema:",
    `        - field_id: ${amountFieldId}`,
    "          field_type: double",
    `        - field_id: ${merchantFieldId}`,
    "          field_type: string",
    "      query:",
    "        filters:",
    `          - field_id: ${amountFieldId}`,
    "            operator: gte",
    "            value:",
    "              variable: minimum_amount",
    "        sort:",
    `          - field_id: ${amountFieldId}`,
    "            direction: asc",
    "        page_limit: 1",
    "        projection:",
    "          kind: fields",
    `          fields: [${amountFieldId}, ${merchantFieldId}]`,
    "    - id: matching_count_source",
    "      kind: saved_sql",
    `      entry_id: "${metricSql.entryId}"`,
    `      revision_id: "${metricSql.revisionId}"`,
    "      expected_result:",
    "        - name: matching_count",
    "          type: integer",
    "      variables:",
    "        minimum:",
    "          variable: minimum_amount",
    "  components:",
    "    - id: minimum_input",
    "      kind: input",
    "      variable: minimum_amount",
    "      label: Minimum amount",
    "    - id: matching_count",
    "      kind: metric",
    "      source: matching_count_source",
    "      column: matching_count",
    "      label: Matching entries",
    "    - id: rows",
    "      kind: table",
    "      source: paged_rows",
    "  layout:",
    "    kind: flow",
    "    rows:",
    "      - id: controls",
    "        items:",
    "          - kind: component",
    "            component: minimum_input",
    "      - id: results",
    "        items:",
    "          - kind: component",
    "            component: matching_count",
    "          - kind: component",
    "            component: rows",
    "",
  ].join("\n");
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
    revisionId: savedComposition.revision_id!,
    compositionName,
  };
}

test.describe("Composition Table and Metric pagination", () => {
  let seed: SeedState;

  test.beforeAll(async ({ request }) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    seed = await seedComposition(request);
  });

  test("Table pagination leaves the SQL Metric and typed Input unchanged", async ({ page }) => {
    test.setTimeout(120_000);
    let metricQueryCount = 0;
    page.on("request", (request) => {
      const url = new URL(request.url());
      if (
        request.method() === "POST" &&
        url.pathname.endsWith(
          `/spaces/${seed.spaceId}/compositions/metric-query`,
        )
      ) {
        metricQueryCount += 1;
      }
    });

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

    const input = page.getByLabel("Minimum amount", { exact: true });
    const metric = page.locator("output.compositionMetric");
    const table = page.getByRole("table");
    await expect(input).toHaveValue("1");
    await expect(metric).toHaveText("2");
    await expect(table.locator("tbody tr")).toHaveCount(1);
    await expect(table).toContainText("Pager-001");
    await expect(page.getByRole("button", { name: "Next", exact: true }))
      .toBeEnabled();
    const initialMetricQueries = metricQueryCount;
    expect(initialMetricQueries).toBeGreaterThan(0);

    await input.fill("0");
    await expect(metric).toHaveText("3");
    await expect(table.locator("tbody tr")).toHaveCount(1);
    await expect(table).toContainText("Pager-000");
    const metricQueriesAfterInputChange = metricQueryCount;
    expect(metricQueriesAfterInputChange).toBeGreaterThan(
      initialMetricQueries,
    );

    await page.getByRole("button", { name: "Next", exact: true }).click();
    await expect(table.locator("tbody tr")).toHaveCount(1);
    await expect(table).toContainText("Pager-001");
    await expect(input).toHaveValue("0");
    await expect(metric).toHaveText("3");
    expect(metricQueryCount).toBe(metricQueriesAfterInputChange);

    await page.getByRole("button", { name: "Previous", exact: true }).click();
    await expect(table.locator("tbody tr")).toHaveCount(1);
    await expect(table).toContainText("Pager-000");
    await expect(input).toHaveValue("0");
    await expect(metric).toHaveText("3");
    expect(metricQueryCount).toBe(metricQueriesAfterInputChange);
  });
});
