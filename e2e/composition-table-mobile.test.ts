import { expect, test } from "@playwright/test";
import {
  getBackendUrl,
  getDefaultSpaceId,
  getFrontendUrl,
} from "./lib/client.ts";
import {
  completeCompositionGraphYaml,
  saveCompositionGraph,
} from "./lib/composition-graph.ts";

test("Composition Table scrolls inside a narrow read-only reader", async ({ page, request }) => {
  test.setTimeout(150_000);
  const spaceId = await getDefaultSpaceId(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `ReaderColumns${suffix}`;
  const columns = [
    "Account Reference",
    "Transaction Category",
    "Entry Date",
    "Quantity",
    "Unit Price",
    "Subtotal",
    "Confirmation",
  ];
  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n${
          columns.map((name) => `## ${name}`).join("\n\n")
        }\n`,
        fields: {
          "Account Reference": { type: "string", required: true },
          "Transaction Category": { type: "string", required: true },
          "Entry Date": { type: "date", required: true },
          Quantity: { type: "integer", required: true },
          "Unit Price": { type: "double", required: true },
          Subtotal: { type: "double", required: true },
          Confirmation: { type: "string", required: true },
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
    fields?: Record<string, { sql_column?: string }>;
  }>;
  const form = forms.find((candidate) => candidate.name === formName);
  expect(form?.id).toBeTruthy();
  expect(form?.sql_relation).toBeTruthy();
  const sqlColumns = columns.map((column) => {
    const sqlColumn = form?.fields?.[column]?.sql_column;
    expect(sqlColumn).toBeTruthy();
    return sqlColumn!;
  });

  const confirmationValue = `verify-${suffix}`;
  const entryResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/entries`),
    {
      data: {
        form: formName,
        fields: {
          "Account Reference": `account-${suffix}`,
          "Transaction Category": "travel",
          "Entry Date": "2026-11-12",
          Quantity: 2,
          "Unit Price": 21,
          Subtotal: 42,
          Confirmation: confirmationValue,
        },
      },
    },
  );
  expect(entryResponse.status()).toBe(201);

  const sqlIdentifier = (value: string) => `"${value.replaceAll('"', '""')}"`;
  const relation = sqlIdentifier(form!.sql_relation!);
  const tableSql = `SELECT ${
    sqlColumns.map((column, index) =>
      `${sqlIdentifier(column)} AS reader_column_${index + 1}`
    ).join(", ")
  } FROM ${relation}`;
  const totalSql = `SELECT ${
    sqlIdentifier(sqlColumns[5])
  } AS total FROM ${relation}`;
  const yaml = await completeCompositionGraphYaml(request, spaceId, {
    name: `Narrow reader ${suffix}`,
    variables: [
      {
        id: "reader_rows",
        kind: "query",
        returns: { kind: "table", columns: [] },
        query: {
          kind: "inline_sql",
          sql: tableSql,
          arguments: [],
          form_bindings: [{
            relation: form!.sql_relation!,
            form_id: form!.id!,
          }],
          schema_fingerprint: "",
        },
      },
      {
        id: "reader_total",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "float", nullable: false },
        },
        query: {
          kind: "inline_sql",
          sql: totalSql,
          arguments: [],
          form_bindings: [{
            relation: form!.sql_relation!,
            form_id: form!.id!,
          }],
          schema_fingerprint: "",
        },
      },
    ],
    components: [
      {
        id: "reader_total_text",
        kind: "text",
        content: "Total: {{reader_total}}",
        style: "body",
      },
      { id: "reader_table", kind: "table", variable: "reader_rows" },
    ],
    rows: [{
      id: "results",
      items: [
        { kind: "component", component: "reader_total_text" },
        { kind: "component", component: "reader_table" },
      ],
    }],
  });
  const saved = await saveCompositionGraph(request, spaceId, yaml);

  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(
    getFrontendUrl(
      `/spaces/${spaceId}/compositions/${saved.composition_id}/${saved.revision_id}`,
    ),
    { waitUntil: "domcontentloaded" },
  );

  const viewport = page.locator(".compositionGraphTableViewport");
  const table = page.getByRole("table", { name: "reader_rows" });
  const total = page.locator(
    '[data-component-id="reader_total_text"] .compositionGraphText',
  );
  await expect(table).toBeVisible({ timeout: 30_000 });
  await expect(total).toHaveText("Total: 42", { timeout: 30_000 });
  await expect(table.getByRole("columnheader")).toHaveCount(columns.length);
  await expect(table.locator("tbody tr")).toHaveCount(1);
  await expect(table.locator("tbody tr").last()).toContainText(
    confirmationValue,
  );
  await expect(page.locator(".compositionGraphInspector")).toHaveCount(0);
  await expect(page.getByRole("tab", { name: "Design", exact: true }))
    .toHaveCount(0);
  await expect(page.getByRole("tab", { name: "Variables", exact: true }))
    .toHaveCount(0);

  for (const width of [320, 375, 390]) {
    await page.setViewportSize({ width, height: 844 });
    await expect(viewport).toBeVisible();
    const dimensions = await page.evaluate(() => ({
      client: document.documentElement.clientWidth,
      content: document.documentElement.scrollWidth,
    }));
    expect(dimensions.content).toBeLessThanOrEqual(dimensions.client);

    const scrollState = await viewport.evaluate((element) => {
      const region = element as HTMLElement;
      return {
        clientWidth: region.clientWidth,
        scrollWidth: region.scrollWidth,
        maxScrollLeft: region.scrollWidth - region.clientWidth,
      };
    });
    expect(scrollState.scrollWidth).toBeGreaterThan(scrollState.clientWidth);

    await viewport.evaluate((element) => {
      const region = element as HTMLElement;
      region.scrollLeft = region.scrollWidth;
    });
    const lastHeader = table.getByRole("columnheader").last();
    const lastCell = table.locator("tbody tr").last().locator("td").last();
    await expect(lastHeader).toBeInViewport();
    await expect(lastCell).toBeInViewport();
    const [regionBounds, headerBounds] = await Promise.all([
      viewport.boundingBox(),
      lastHeader.boundingBox(),
    ]);
    expect(regionBounds).toBeTruthy();
    expect(headerBounds).toBeTruthy();
    expect(headerBounds!.x).toBeGreaterThanOrEqual(regionBounds!.x - 1);
    expect(headerBounds!.x + headerBounds!.width).toBeLessThanOrEqual(
      regionBounds!.x + regionBounds!.width + 1,
    );
    await expect(total).toBeVisible();
    await expect(total).toHaveText("Total: 42");
  }
});
