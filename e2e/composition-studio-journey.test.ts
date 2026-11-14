import {
  type APIRequestContext,
  type Browser,
  type BrowserContext,
  expect,
  type Page,
  type Response,
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
  saveCompositionGraph,
} from "./lib/composition-graph.ts";
import {
  type CompositionFormSeedField,
  expectedCompositionFormSeedSql,
  normalizeCompositionFormSeedSql,
} from "./lib/composition-form.ts";
import { parse as parseYaml } from "yaml";

type StudioSeedState = {
  spaceId: string;
  formName: string;
  formId: string;
  formRelation: string;
  toolName: string;
  monthStart: string;
  monthEnd: string;
  tableSql: string;
  totalSql: string;
  quantitySql: string;
  expectedTotal: string;
  expectedQuantity: string;
  purposes: string[];
};

type ReactiveTextSeed = {
  spaceId: string;
  formName: string;
  formId: string;
  relation: string;
  toolName: string;
  inlineSql: string;
};

type StudioHistoryPage = {
  revisions: Array<{ revision: { revision_id: string } }>;
  total: number;
};

type StudioRawRevision = {
  revision: { entry_id: string; revision_id: string };
  fields: Record<string, unknown>;
};

type ListedStudioEntry = { id?: string; form?: string };

async function snapshotStudioEntryIds(
  request: APIRequestContext,
  spaceId: string,
): Promise<string[]> {
  const response = await request.get(
    getBackendUrl(`/spaces/${spaceId}/entries?limit=1000&offset=0`),
  );
  expect(response.ok()).toBe(true);
  const entries = await response.json() as ListedStudioEntry[];
  return entries.map(({ id, form }) => `${form ?? ""}:${id ?? ""}`).sort();
}

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

async function createSpace(request: APIRequestContext): Promise<string> {
  return await getDefaultSpaceId(request);
}

async function seedStudioKnowledge(
  request: APIRequestContext,
): Promise<StudioSeedState> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `StudioExpense${suffix}`;
  const toolName = `Studio monthly tool ${suffix}`;
  const monthStart = "2026-01-01";
  const monthEnd = "2026-02-01";
  // Exactly one January row lets each scalar Text reference resolve to one
  // value while the Table query exposes both source columns.
  const purposes = ["Studio Jan one", "Studio Feb one"];
  const entries = [
    { date: "2026-01-15", purpose: purposes[0], amount: 20, quantity: 3 },
    { date: "2026-02-05", purpose: purposes[1], amount: 30, quantity: 5 },
  ];

  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}

## Date

## Purpose

## Amount

## Quantity
`,
        fields: {
          Date: { type: "date", required: true },
          Purpose: { type: "string", required: true },
          Amount: { type: "double", required: true },
          Quantity: { type: "double", required: true },
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
    fields?: Record<string, CompositionFormSeedField>;
  }>;
  const form = forms.find((candidate) => candidate.name === formName);
  expect(form?.id).toBeTruthy();
  expect(form?.sql_relation).toBeTruthy();
  const dateColumn = form!.fields?.Date?.sql_column;
  const purposeColumn = form!.fields?.Purpose?.sql_column;
  const amountColumn = form!.fields?.Amount?.sql_column;
  const quantityColumn = form!.fields?.Quantity?.sql_column;
  expect(dateColumn).toBeTruthy();
  expect(purposeColumn).toBeTruthy();
  expect(amountColumn).toBeTruthy();
  expect(quantityColumn).toBeTruthy();

  for (const entry of entries) {
    const entryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      {
        data: {
          form: formName,
          fields: {
            Date: entry.date,
            Purpose: entry.purpose,
            Amount: entry.amount,
            Quantity: entry.quantity,
          },
        },
      },
    );
    expect(entryResponse.status()).toBe(201);
  }

  // Inline SQL is the Composition source. The standalone Saved SQL APIs have
  // independent coverage below.
  const tableSql = `SELECT ${sqlIdentifier(amountColumn!)} AS total, ${
    sqlIdentifier(quantityColumn!)
  } AS quantity FROM ${sqlIdentifier(form!.sql_relation!)} WHERE ${
    sqlIdentifier(dateColumn!)
  } >= $month_start AND ${sqlIdentifier(dateColumn!)} < $month_end ORDER BY ${
    sqlIdentifier(dateColumn!)
  } ASC`;
  const filter = `FROM ${sqlIdentifier(form!.sql_relation!)} WHERE ${
    sqlIdentifier(dateColumn!)
  } >= $month_start AND ${sqlIdentifier(dateColumn!)} < $month_end`;
  const totalSql = `SELECT ${
    sqlIdentifier(amountColumn!)
  } AS total ${filter} ORDER BY ${sqlIdentifier(dateColumn!)} ASC`;
  const quantitySql = `SELECT ${
    sqlIdentifier(quantityColumn!)
  } AS quantity ${filter} ORDER BY ${sqlIdentifier(dateColumn!)} ASC`;

  return {
    spaceId,
    formName,
    formId: form!.id!,
    formRelation: form!.sql_relation!,
    toolName,
    monthStart,
    monthEnd,
    tableSql,
    totalSql,
    quantitySql,
    expectedTotal: "20",
    expectedQuantity: "3",
    purposes,
  };
}

async function seedReusableComposition(
  request: APIRequestContext,
  seed: StudioSeedState,
): Promise<{ composition_id: string; revision_id: string }> {
  const inlineQuery = (id: string, sql: string, returns: unknown) => ({
    id,
    kind: "query" as const,
    returns,
    query: {
      kind: "inline_sql" as const,
      sql,
      arguments: [
        { name: "month_start", variable: "month_start" },
        { name: "month_end", variable: "month_end" },
      ],
      form_bindings: [{
        relation: seed.formRelation,
        form_id: seed.formId,
      }],
      schema_fingerprint: "",
    },
  });
  const graph: CompositionGraphSeed = {
    name: seed.toolName,
    variables: [
      {
        id: "month_start",
        kind: "input",
        value_type: { scalar_type: "date", nullable: false },
        required: true,
        default: { kind: "date", value: seed.monthStart },
      },
      {
        id: "month_end",
        kind: "input",
        value_type: { scalar_type: "date", nullable: false },
        required: true,
        default: { kind: "date", value: seed.monthEnd },
      },
      {
        id: "report_title",
        kind: "input",
        value_type: { scalar_type: "string", nullable: false },
        required: true,
        default: { kind: "string", value: "January expenses" },
      },
      inlineQuery("studio_rows", seed.tableSql, { kind: "table", columns: [] }),
      inlineQuery("studio_total", seed.totalSql, {
        kind: "scalar",
        value_type: { scalar_type: "float", nullable: false },
      }),
      inlineQuery("studio_quantity", seed.quantitySql, {
        kind: "scalar",
        value_type: { scalar_type: "float", nullable: false },
      }),
    ],
    components: [
      { id: "month_start_input", kind: "input", variable: "month_start" },
      { id: "month_end_input", kind: "input", variable: "month_end" },
      { id: "report_title_input", kind: "input", variable: "report_title" },
      {
        id: "report_title_text",
        kind: "text",
        content: "{{report_title}}",
        style: "body",
      },
      {
        id: "total_text",
        kind: "text",
        content: "{{studio_total}}",
        style: "body",
      },
      {
        id: "quantity_text",
        kind: "text",
        content: "{{studio_quantity}}",
        style: "body",
      },
      { id: "rows", kind: "table", variable: "studio_rows" },
    ],
    rows: [
      {
        id: "inputs",
        items: ["month_start_input", "month_end_input", "report_title_input"]
          .map((component) => ({
            kind: "component" as const,
            component,
          })),
      },
      {
        id: "results",
        items: ["report_title_text", "total_text", "quantity_text", "rows"].map(
          (component) => ({
            kind: "component" as const,
            component,
          }),
        ),
      },
    ],
  };
  const yaml = await completeCompositionGraphYaml(request, seed.spaceId, graph);
  return await saveCompositionGraph(request, seed.spaceId, yaml);
}

async function seedReactiveTextKnowledge(
  request: APIRequestContext,
): Promise<ReactiveTextSeed> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `FilteredExpense${suffix}`;
  const toolName = `Filtered expense dashboard ${suffix}`;

  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Amount\n`,
        fields: { Amount: { type: "double", required: true } },
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
    fields?: Record<string, CompositionFormSeedField>;
  }>;
  const form = forms.find((candidate) => candidate.name === formName);
  expect(form?.id).toBeTruthy();
  expect(form?.sql_relation).toBeTruthy();
  const amountColumn = form?.fields?.Amount?.sql_column;
  expect(amountColumn).toBeTruthy();

  for (const amount of [10, 20, 30]) {
    const entryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      { data: { form: formName, fields: { Amount: amount } } },
    );
    expect(entryResponse.status()).toBe(201);
  }

  const amount = sqlIdentifier(amountColumn!);
  const inlineSql = `SELECT COUNT(*) AS matching_count FROM ${
    sqlIdentifier(form!.sql_relation!)
  } WHERE ${amount} >= $minimum`;

  return {
    spaceId,
    formName,
    formId: form!.id!,
    relation: form!.sql_relation!,
    toolName,
    inlineSql,
  };
}

async function expectHistoryTotal(
  request: APIRequestContext,
  spaceId: string,
  compositionId: string,
  total: number,
): Promise<StudioHistoryPage> {
  const response = await request.get(
    getBackendUrl(
      `/spaces/${spaceId}/compositions/${compositionId}/history?limit=100&offset=0`,
    ),
  );
  expect(response.ok()).toBe(true);
  const history = await response.json() as StudioHistoryPage;
  expect(history.total).toBe(total);
  expect(history.revisions).toHaveLength(total);
  return history;
}

function revisionFromUrl(url: string): {
  compositionId: string;
  revisionId: string;
} {
  const match = url.match(/\/compositions\/([^/]+)\/([^/]+)$/);
  expect(match).toBeTruthy();
  return { compositionId: match![1], revisionId: match![2] };
}

async function openToolFromSavedTools(
  page: Page,
  spaceId: string,
  compositionId: string,
  revisionId: string,
  toolName: string,
): Promise<string> {
  const path = `/spaces/${spaceId}/compositions/${compositionId}/${revisionId}`;
  const compositionList = page.waitForResponse((response) => {
    const url = new URL(response.url());
    return response.request().method() === "GET" &&
      url.pathname === `/api/spaces/${spaceId}/compositions` &&
      url.searchParams.get("limit") === "50" &&
      url.searchParams.get("offset") === "0";
  });
  await page.goto(getFrontendUrl(`/spaces/${spaceId}/compositions`), {
    waitUntil: "domcontentloaded",
  });
  const listedCompositions = await compositionList;
  expect(listedCompositions.ok()).toBe(true);
  const compositionPage = await listedCompositions.json() as {
    items?: Array<{ composition_id?: string; revision_id?: string }>;
  };
  expect(
    compositionPage.items?.some((item) =>
      item.composition_id === compositionId && item.revision_id === revisionId
    ),
  ).toBe(true);
  const link = page.locator(`a[href="${path}"]`);
  await expect(link).toBeVisible();
  const revisionResponse = page.waitForResponse((response) =>
    response.request().method() === "GET" &&
    new URL(response.url()).pathname.endsWith(
      `/spaces/${spaceId}/compositions/${compositionId}/history/${revisionId}`,
    )
  );
  await link.click();
  await expect(page).toHaveURL(new RegExp(`${path.replaceAll("/", "\\/")}$`));
  const loadedRevision = await revisionResponse;
  expect(loadedRevision.ok()).toBe(true);
  const revisionBody = await loadedRevision.json() as StudioRawRevision;
  expect(revisionBody.revision.entry_id).toBe(compositionId);
  expect(revisionBody.revision.revision_id).toBe(revisionId);
  await expect(
    page.getByRole("heading", { level: 1, name: toolName }),
  ).toBeVisible();
  return path;
}

function isDraftQueryEvaluation(
  response: Response,
  variableId: string,
  sql: string,
): boolean {
  if (
    !new URL(response.url()).pathname.endsWith("/compositions/evaluate") ||
    response.request().method() !== "POST"
  ) return false;
  const body = response.request().postDataJSON() as {
    target?: { kind?: string; yaml?: string };
    requested_queries?: string[];
  };
  return body.target?.kind === "draft" &&
    body.target.yaml?.includes(`id: ${variableId}`) === true &&
    body.target.yaml?.includes(sql) === true &&
    body.requested_queries?.includes(variableId) === true;
}

type CompositionSemanticValues = {
  inputs: Record<string, string>;
  text: string[];
  table: { headings: string[]; rows: string[][] };
};

function normalizeSemanticText(value: string): string {
  return value.replace(/\s+/g, " ").trim();
}

async function expectCompositionSemanticValues(
  page: Page,
  expected: CompositionSemanticValues,
): Promise<void> {
  const table = page.locator(".compositionGraphTableViewport")
    .getByRole("table");
  await expect(table).toBeVisible({ timeout: 30_000 });
  await expect(table.locator("tbody tr")).toHaveCount(
    expected.table.rows.length,
    { timeout: 30_000 },
  );

  const inputs = Object.fromEntries(
    await Promise.all(
      Object.keys(expected.inputs).map(async (name) => [
        name,
        await page.getByLabel(name, { exact: true })
          .and(page.locator("input")).inputValue(),
      ]),
    ),
  );
  const headings = (await table.getByRole("columnheader").allTextContents())
    .map(normalizeSemanticText);
  const rows = await table.locator("tbody tr").evaluateAll((tableRows) =>
    tableRows.map((row) =>
      Array.from(row.querySelectorAll("td"), (cell) => cell.textContent ?? "")
    )
  );
  const text = (await page.locator(
    ".compositionGraphRender .compositionGraphValue :is(p, h2).compositionGraphText",
  ).allTextContents()).map(normalizeSemanticText);

  expect({
    inputs,
    text,
    table: {
      headings,
      rows: rows.map((row) => row.map(normalizeSemanticText)),
    },
  }).toEqual(expected);
}

async function openOwnerContext(
  browser: Browser,
  storageState: Awaited<ReturnType<BrowserContext["storageState"]>>,
) {
  return await browser.newContext({ storageState });
}

test.describe("Composition Studio Journey", () => {
  let seed: StudioSeedState;

  test.beforeAll(async ({ request }) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    seed = await seedStudioKnowledge(request);
  });

  test("Composition Studio saves, reopens, and restores an inline SQL Text tool", async ({ browser, context, request }) => {
    test.setTimeout(180_000);
    const storageState = await context.storageState();
    const saved = await seedReusableComposition(request, seed);
    const compositionId = saved.composition_id;
    const firstRevisionId = saved.revision_id;
    let editedRevisionId = "";
    await expectHistoryTotal(request, seed.spaceId, compositionId, 1);

    const readerContext = await openOwnerContext(browser, storageState);
    try {
      const page = await readerContext.newPage();
      const evaluatePayloads: Array<Record<string, unknown>> = [];
      page.on("request", (request) => {
        if (
          !new URL(request.url()).pathname.endsWith("/compositions/evaluate")
        ) {
          return;
        }
        try {
          evaluatePayloads.push(
            request.postDataJSON() as Record<string, unknown>,
          );
        } catch {
          evaluatePayloads.push({});
        }
      });
      await openToolFromSavedTools(
        page,
        seed.spaceId,
        compositionId,
        firstRevisionId,
        seed.toolName,
      );
      await expect(page.getByRole("tablist", { name: "Studio mode" }))
        .toHaveCount(0);
      await expect(page.getByRole("group", { name: "Add to design" }))
        .toHaveCount(0);
      await expect(page.locator("#composition-text-editor-report_title_text"))
        .toHaveCount(0);
      const semanticValues: CompositionSemanticValues = {
        inputs: {
          month_start: seed.monthStart,
          month_end: seed.monthEnd,
          report_title: "January expenses",
        },
        text: ["January expenses", seed.expectedTotal, seed.expectedQuantity],
        table: {
          headings: ["total", "quantity"],
          rows: [[seed.expectedTotal, seed.expectedQuantity]],
        },
      };
      await expectCompositionSemanticValues(page, semanticValues);

      const initialEvaluationCount = evaluatePayloads.length;
      await page.getByLabel("report_title", { exact: true }).fill(
        "Transient report title",
      );
      await expect(page.locator(
        '[data-component-id="report_title_text"] :is(p, h2).compositionGraphText',
      )).toHaveText("Transient report title");
      await expect.poll(() => evaluatePayloads.length).toBeGreaterThan(
        initialEvaluationCount,
      );
      expect(evaluatePayloads.at(-1)?.requested_queries).toEqual([]);

      await page.getByLabel("month_start", { exact: true }).fill("2026-02-01");
      await page.getByLabel("month_end", { exact: true }).fill("2026-03-01");
      await expect(page.getByLabel("month_start", { exact: true }))
        .toHaveValue("2026-02-01");
      await expect(page.getByLabel("month_end", { exact: true }))
        .toHaveValue("2026-03-01");
      await expect(page.locator(
        '[data-component-id="total_text"] :is(p, h2).compositionGraphText',
      )).toHaveText("30", { timeout: 30_000 });
      await expect(page.locator(
        '[data-component-id="quantity_text"] :is(p, h2).compositionGraphText',
      )).toHaveText("5", { timeout: 30_000 });
      const changedTable = page.getByRole("table", { name: "studio_rows" });
      await expect(changedTable.getByRole("columnheader")).toHaveText([
        "total",
        "quantity",
      ]);
      await expect(changedTable.locator("tbody tr")).toHaveCount(1);
      await expect(
        changedTable.locator("tbody tr").getByRole("cell"),
      ).toHaveText(["30", "5"]);
      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(page.getByLabel("month_start", { exact: true })).toHaveValue(
        seed.monthStart,
      );
      await expectCompositionSemanticValues(page, semanticValues);
      await expectHistoryTotal(request, seed.spaceId, compositionId, 1);

      await page.goto(
        getFrontendUrl(
          `/spaces/${seed.spaceId}/compositions/${compositionId}/${firstRevisionId}/edit`,
        ),
        { waitUntil: "domcontentloaded" },
      );
      await expect(page.getByRole("tab", { name: "Design", exact: true }))
        .toBeVisible({ timeout: 30_000 });
      await expect(page.getByLabel("Name", { exact: true })).toHaveValue(
        seed.toolName,
      );
      const textBlock = page.locator(
        '.compositionGraphDesignValue[data-component-id="report_title_text"] button.compositionGraphTextSelect',
      );
      await textBlock.click();
      const textEditor = page.locator(
        "#composition-text-editor-report_title_text",
      );
      await expect(textEditor).toHaveValue("{{report_title}}");
      await textEditor.fill("Report: {{report_title}}");
      await page.getByRole("button", { name: "Save", exact: true }).click();
      await expect(page).toHaveURL(
        new RegExp(
          `/spaces/${seed.spaceId}/compositions/${compositionId}/[^/]+$`,
        ),
        { timeout: 30_000 },
      );
      const edited = revisionFromUrl(page.url());
      expect(edited.compositionId).toBe(compositionId);
      expect(edited.revisionId).not.toBe(firstRevisionId);
      editedRevisionId = edited.revisionId;
      await expect(page.locator(
        '[data-component-id="report_title_text"] :is(p, h2).compositionGraphText',
      )).toHaveText("Report: January expenses");
      await expect(page.locator(
        '[data-component-id="total_text"] :is(p, h2).compositionGraphText',
      )).toHaveText(seed.expectedTotal);
      await expectHistoryTotal(request, seed.spaceId, compositionId, 2);

      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(page.locator(
        '[data-component-id="report_title_text"] :is(p, h2).compositionGraphText',
      )).toHaveText("Report: January expenses");

      await page.goto(
        getFrontendUrl(
          `/spaces/${seed.spaceId}/compositions/${compositionId}/history`,
        ),
        { waitUntil: "domcontentloaded" },
      );
      const createdLink = page.getByRole("link", { name: /^Created/ });
      await expect(createdLink).toBeVisible();
      await expect(page.getByRole("link", { name: /^Edited/ })).toBeVisible();
      await createdLink.click();
      await expect(page).toHaveURL(
        new RegExp(
          `/spaces/${seed.spaceId}/compositions/${compositionId}/${firstRevisionId}$`,
        ),
      );
      await page.getByRole("button", {
        name: "Restore this revision",
        exact: true,
      }).click();
      await expect.poll(() => page.url(), { timeout: 30_000 })
        .not.toContain(`/${firstRevisionId}`);
      const restored = revisionFromUrl(page.url());
      expect(restored.compositionId).toBe(compositionId);
      expect(restored.revisionId).not.toBe(firstRevisionId);
      expect(restored.revisionId).not.toBe(editedRevisionId);
      await expectHistoryTotal(request, seed.spaceId, compositionId, 3);
      await expect(page.locator(
        '[data-component-id="report_title_text"] :is(p, h2).compositionGraphText',
      )).toHaveText("January expenses", { timeout: 30_000 });
      await expect(page.locator(
        '[data-component-id="total_text"] :is(p, h2).compositionGraphText',
      )).toHaveText(seed.expectedTotal, { timeout: 30_000 });
      await expect(page.getByRole("heading", { name: "total", exact: true }))
        .toHaveCount(0);
    } finally {
      await readerContext.close();
    }
  });

  test("Composition Studio creates Form-backed SQL and edits a plain Text component", async ({ browser, context, request }) => {
    test.setTimeout(180_000);
    const formsResponse = await request.get(
      getBackendUrl(`/spaces/${seed.spaceId}/forms`),
    );
    expect(formsResponse.ok()).toBe(true);
    const forms = await formsResponse.json() as Array<{
      id?: string;
      name?: string;
      sql_relation?: string;
      fields?: Record<string, CompositionFormSeedField>;
    }>;
    const form = forms.find((candidate) => candidate.name === seed.formName);
    expect(form?.id).toBeTruthy();
    const amountColumn = form?.fields?.Amount?.sql_column;
    expect(amountColumn).toBeTruthy();
    const formSlug = seed.formName.normalize("NFKD").toLowerCase()
      .replace(/\p{M}/gu, "")
      .replace(/[^a-z0-9]+/g, "_")
      .replace(/^_+|_+$/g, "");
    const rowsVariable = `${formSlug}_rows`;
    const scalarVariable = `${formSlug}_sum_amount`;
    expect(form?.sql_relation).toBeTruthy();
    const tableSql = expectedCompositionFormSeedSql(
      form!.sql_relation!,
      form?.fields,
    );
    const scalarSql = `SELECT SUM(${
      sqlIdentifier(amountColumn!)
    }) AS value FROM ${sqlIdentifier(form!.sql_relation!)}`;

    const ownerContext = await openOwnerContext(
      browser,
      await context.storageState(),
    );
    try {
      const page = await ownerContext.newPage();
      await page.goto(
        getFrontendUrl(`/spaces/${seed.spaceId}/compositions/new`),
        { waitUntil: "domcontentloaded" },
      );
      await expect(page.getByPlaceholder("Composition name")).toBeVisible();
      await expect(page.getByRole("complementary", { name: "Inspector" }))
        .toHaveCount(0);
      for (const name of ["Table", "Input", "Text"]) {
        await expect(
          page.getByRole("group", { name: "Add to design" }).getByRole(
            "button",
            { name, exact: true },
          ),
        ).toBeVisible();
      }
      await page.getByLabel("Name", { exact: true }).fill(
        `${seed.toolName} SQL Text`,
      );
      const palette = page.getByRole("group", { name: "Add to design" });
      await palette.getByRole("button", { name: "Table", exact: true }).click();
      const tableNameField = page.getByLabel("Variable name", { exact: true });
      await expect(tableNameField).toBeVisible();
      await tableNameField.fill(rowsVariable);
      await page.getByRole("button", { name: "Add", exact: true }).click();
      await page.getByRole("tab", { name: "Variables", exact: true }).click();
      await page.getByRole("button", { name: rowsVariable, exact: true })
        .click();
      const tableEditor = page.getByRole("textbox", {
        name: "SQL",
        exact: true,
      });
      await tableEditor.fill(tableSql);
      const tablePreviewResponse = page.waitForResponse((response) =>
        isDraftQueryEvaluation(response, rowsVariable, tableSql)
      );
      await page.getByRole("button", { name: "Run Query", exact: true })
        .click();
      const tablePreview = await (await tablePreviewResponse).json() as {
        ok?: boolean;
        queries?: Array<{ variable_id?: string; schema_fingerprint?: string }>;
      };
      expect(tablePreview.ok).toBe(true);
      expect(
        tablePreview.queries?.find((query) =>
          query.variable_id === rowsVariable
        )
          ?.schema_fingerprint,
      ).toMatch(/^[0-9a-f]{64}$/);

      await page.getByRole("button", { name: "Add variable", exact: true })
        .click();
      await page.getByRole("group", { name: "Add variable" })
        .getByRole("button", { name: "SQL", exact: true }).click();
      const scalarNameField = page.getByLabel("Variable name", { exact: true });
      await scalarNameField.fill(scalarVariable);
      await page.getByRole("button", { name: "Add", exact: true }).click();
      await page.getByRole("tab", { name: "Variables", exact: true }).click();
      await page.getByRole("button", { name: scalarVariable, exact: true })
        .click();
      await page.getByRole("button", { name: "Form", exact: true }).click();
      await page.getByRole("group", { name: "Available Forms", exact: true })
        .getByRole("button", { name: seed.formName, exact: true }).click();
      await page.getByRole("textbox", { name: "SQL", exact: true })
        .fill(scalarSql);
      const scalarPreviewResponse = page.waitForResponse((response) =>
        isDraftQueryEvaluation(response, scalarVariable, scalarSql)
      );
      await page.getByRole("button", { name: "Run Query", exact: true })
        .click();
      const scalarPreview = await (await scalarPreviewResponse).json() as {
        ok?: boolean;
        query_results?: Record<string, { value?: { kind?: string } }>;
      };
      expect(scalarPreview.ok).toBe(true);
      expect(scalarPreview.query_results?.[scalarVariable]?.value?.kind)
        .toBe("scalar");

      await page.getByRole("tab", { name: "Design", exact: true }).click();
      await palette.getByRole("button", { name: "Text", exact: true }).click();
      const textEditor = page.locator("#composition-text-editor-text_1");
      await expect(textEditor).toBeVisible();
      await textEditor.fill(`Total: {{${scalarVariable}}}`);
      const textStyle = page.getByRole("combobox", {
        name: "Text style",
        exact: true,
      });
      await textStyle.selectOption("heading");
      const designTable = page.getByRole("table", { name: rowsVariable });
      await expect(designTable).toContainText(seed.purposes[0], {
        timeout: 30_000,
      });
      await expect(designTable).toContainText(seed.purposes[1]);
      await expect(page.locator(
        '[data-component-id="text_1"] :is(p, h2).compositionGraphText',
      )).toHaveText("Total: 50", { timeout: 30_000 });
      expect(
        await page.evaluate(() =>
          document.documentElement.scrollWidth <= window.innerWidth
        ),
      ).toBe(true);

      await page.getByRole("button", { name: "Save", exact: true }).click();
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${seed.spaceId}/compositions/[^/]+/[^/]+$`),
        { timeout: 30_000 },
      );
      const saved = revisionFromUrl(page.url());
      const response = await request.get(
        getBackendUrl(
          `/spaces/${seed.spaceId}/compositions/${saved.compositionId}/history/${saved.revisionId}`,
        ),
      );
      expect(response.ok()).toBe(true);
      const revision = await response.json() as StudioRawRevision;
      const savedDocument = parseYaml(String(revision.fields["spec"])) as {
        spec?: {
          variables?: Array<{
            id?: string;
            query?: {
              kind?: string;
              sql?: string;
              form_bindings?: Array<{ relation: string; form_id: string }>;
            };
          }>;
          components?: Array<
            Record<string, unknown> & {
              id?: string;
              kind?: string;
              content?: string;
            }
          >;
          layout?: { rows?: Array<{ items?: Array<{ component?: string }> }> };
        };
      };
      const savedRows = savedDocument.spec?.variables?.find((item) =>
        item.id === rowsVariable
      );
      expect(savedRows?.query?.kind).toBe("inline_sql");
      expect(normalizeCompositionFormSeedSql(savedRows?.query?.sql ?? ""))
        .toBe(tableSql);
      expect(savedRows?.query?.form_bindings).toEqual([{
        relation: form!.sql_relation,
        form_id: form!.id!,
      }]);
      const savedScalar = savedDocument.spec?.variables?.find((item) =>
        item.id === scalarVariable
      );
      expect(savedScalar?.query?.kind).toBe("inline_sql");
      expect(savedScalar?.query?.sql).toBe(scalarSql);
      const savedText = savedDocument.spec?.components?.find((component) =>
        component.id === "text_1"
      );
      expect(savedText).toMatchObject({
        kind: "text",
        content: `Total: {{${scalarVariable}}}`,
        style: "heading",
      });
      expect(savedText).not.toHaveProperty("presentation");
      expect(
        savedDocument.spec?.components?.every((component) =>
          component.kind === "table" || component.kind === "text"
        ),
      ).toBe(true);

      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(page.getByRole("table", { name: rowsVariable }))
        .toContainText(seed.purposes[0], { timeout: 30_000 });
      await expect(page.locator(
        '[data-component-id="text_1"] :is(p, h2).compositionGraphText',
      )).toHaveText("Total: 50", { timeout: 30_000 });
    } finally {
      await ownerContext.close();
    }
  });

  test("A06: a Form-backed scalar query renders in a plain Text component", async ({ page, request }) => {
    test.setTimeout(120_000);
    const formsResponse = await request.get(
      getBackendUrl(`/spaces/${seed.spaceId}/forms`),
    );
    expect(formsResponse.ok()).toBe(true);
    const forms = await formsResponse.json() as Array<{
      id?: string;
      name?: string;
      sql_relation?: string;
      fields?: Record<string, CompositionFormSeedField>;
    }>;
    const form = forms.find((candidate) => candidate.name === seed.formName);
    expect(form?.id).toBeTruthy();
    const amountColumn = form?.fields?.Amount?.sql_column;
    expect(amountColumn).toBeTruthy();
    const averageSql = `SELECT AVG(${
      sqlIdentifier(amountColumn!)
    }) AS value FROM ${sqlIdentifier(form!.sql_relation!)}`;
    const yaml = await completeCompositionGraphYaml(request, seed.spaceId, {
      name: `${seed.toolName} average`,
      variables: [{
        id: "average_amount",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "float", nullable: false },
        },
        query: {
          kind: "inline_sql",
          sql: averageSql,
          arguments: [],
          form_bindings: [{
            relation: form!.sql_relation!,
            form_id: form!.id!,
          }],
          schema_fingerprint: "",
        },
      }],
      components: [{
        id: "average_text",
        kind: "text",
        content: "Average: {{average_amount}}",
        style: "body",
      }],
      rows: [{
        id: "results",
        items: [{ kind: "component", component: "average_text" }],
      }],
    });
    const saved = await saveCompositionGraph(request, seed.spaceId, yaml);
    await page.goto(
      getFrontendUrl(
        `/spaces/${seed.spaceId}/compositions/${saved.composition_id}/${saved.revision_id}`,
      ),
      { waitUntil: "domcontentloaded" },
    );
    await expect(page.locator(
      '[data-component-id="average_text"] :is(p, h2).compositionGraphText',
    )).toHaveText("Average: 25", { timeout: 30_000 });
    const response = await request.get(
      getBackendUrl(
        `/spaces/${seed.spaceId}/compositions/${saved.composition_id}/history/${saved.revision_id}`,
      ),
    );
    expect(response.ok()).toBe(true);
    const revision = await response.json() as StudioRawRevision;
    const document = parseYaml(String(revision.fields["spec"])) as {
      spec?: {
        variables?: Array<
          { query?: { kind?: string; sql?: string; form_bindings?: unknown[] } }
        >;
      };
    };
    expect(document.spec?.variables?.[0]?.query).toMatchObject({
      kind: "inline_sql",
      sql: averageSql,
      form_bindings: [{
        relation: form!.sql_relation,
        form_id: form!.id!,
      }],
    });
  });

  test("Studio creates a Form-backed Table query from Design and reopens its result", async ({ page, request }) => {
    test.setTimeout(150_000);
    const spaceId = await createSpace(request);
    const suffix = crypto.randomUUID().slice(0, 8);
    const formName = `StudioTable${suffix}`;
    const formResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/forms`),
      {
        data: {
          name: formName,
          version: 1,
          template: `# ${formName}\n\n## Name\n`,
          fields: { Name: { type: "string", required: true } },
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
      fields?: Record<string, CompositionFormSeedField>;
    }>;
    const form = forms.find((candidate) => candidate.name === formName);
    expect(form?.id).toBeTruthy();
    expect(form?.sql_relation).toBeTruthy();
    const nameColumn = form?.fields?.Name?.sql_column;
    expect(nameColumn).toBeTruthy();

    await page.goto(getFrontendUrl(`/spaces/${spaceId}/compositions/new`), {
      waitUntil: "domcontentloaded",
    });
    await page.getByLabel("Name", { exact: true }).fill(`Table ${suffix}`);
    const palette = page.getByRole("group", { name: "Add to design" });
    await palette.getByRole("button", { name: "Table", exact: true }).click();
    const variableName = page.getByLabel("Variable name", { exact: true });
    await expect(variableName).toBeVisible();
    await variableName.fill("studio_rows");
    await page.getByRole("button", { name: "Add", exact: true }).click();

    await page.getByRole("tab", { name: "Variables", exact: true }).click();
    await page.getByRole("button", { name: "studio_rows", exact: true })
      .click();
    await page.getByRole("button", { name: "Form", exact: true }).click();
    const availableForms = page.getByRole("group", {
      name: "Available Forms",
      exact: true,
    });
    await availableForms.getByRole("button", { name: formName, exact: true })
      .click();
    const sql = `SELECT ${sqlIdentifier(nameColumn!)} AS name FROM ${
      sqlIdentifier(form!.sql_relation!)
    }`;
    const editor = page.getByRole("textbox", { name: "SQL", exact: true });
    await editor.fill(sql);
    const emptyPreview = page.waitForResponse((response) =>
      isDraftQueryEvaluation(response, "studio_rows", sql)
    );
    await page.getByRole("button", { name: "Run Query", exact: true }).click();
    expect((await (await emptyPreview).json() as { ok?: boolean }).ok).toBe(
      true,
    );
    await page.getByRole("tab", { name: "Design", exact: true }).click();
    const resultTable = page.getByRole("table", { name: "studio_rows" });
    await expect(resultTable).toBeVisible();
    await expect(resultTable.locator("tbody tr")).toHaveCount(0);

    const entryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      { data: { form: formName, fields: { Name: `Reopened row ${suffix}` } } },
    );
    expect(entryResponse.status()).toBe(201);
    await page.getByRole("tab", { name: "Variables", exact: true }).click();
    await page.getByRole("button", { name: "Run Query", exact: true }).click();
    await page.getByRole("tab", { name: "Design", exact: true }).click();
    await expect(resultTable).toContainText(`Reopened row ${suffix}`, {
      timeout: 30_000,
    });

    await page.getByRole("button", { name: "Save", exact: true }).click();
    await expect(page).toHaveURL(/\/compositions\/[^/]+\/[^/]+$/, {
      timeout: 30_000,
    });
    const saved = revisionFromUrl(page.url());
    const savedResponse = await request.get(
      getBackendUrl(
        `/spaces/${spaceId}/compositions/${saved.compositionId}/history/${saved.revisionId}`,
      ),
    );
    expect(savedResponse.ok()).toBe(true);
    const savedRevision = await savedResponse.json() as StudioRawRevision;
    const savedSpec = parseYaml(String(savedRevision.fields["spec"])) as {
      spec?: {
        variables?: Array<{
          id?: string;
          query?: {
            kind?: string;
            sql?: string;
            form_bindings?: Array<{ relation: string; form_id: string }>;
          };
        }>;
      };
    };
    expect(
      savedSpec.spec?.variables?.find((variable) =>
        variable.id === "studio_rows"
      )?.query,
    ).toMatchObject({
      kind: "inline_sql",
      sql,
      form_bindings: [{ relation: form!.sql_relation, form_id: form!.id }],
    });
    await page.reload({ waitUntil: "domcontentloaded" });
    await expect(page.getByRole("tablist", { name: "Studio mode" }))
      .toHaveCount(0);
    await expect(page.getByRole("group", { name: "Add to design" }))
      .toHaveCount(0);
    await expect(page.getByRole("table", { name: "studio_rows" }))
      .toContainText(`Reopened row ${suffix}`);
  });

  test("Inline SQL preview refreshes the dependent Text value when an Input changes", async ({ page, request }) => {
    test.setTimeout(120_000);
    const seed = await seedReactiveTextKnowledge(request);
    const yaml = await completeCompositionGraphYaml(request, seed.spaceId, {
      name: seed.toolName,
      variables: [
        {
          id: "minimum",
          kind: "input",
          value_type: { scalar_type: "integer", nullable: false },
          required: true,
          default: { kind: "integer", value: 15 },
        },
        {
          id: "matching_count",
          kind: "query",
          returns: {
            kind: "scalar",
            value_type: { scalar_type: "integer", nullable: false },
          },
          query: {
            kind: "inline_sql",
            sql: seed.inlineSql,
            arguments: [{ name: "minimum", variable: "minimum" }],
            form_bindings: [{
              relation: seed.relation,
              form_id: seed.formId,
            }],
            schema_fingerprint: "",
          },
        },
      ],
      components: [
        { id: "minimum_input", kind: "input", variable: "minimum" },
        {
          id: "matching_text",
          kind: "text",
          content: "Matches: {{matching_count}}",
          style: "body",
        },
      ],
      rows: [{
        id: "results",
        items: [
          { kind: "component", component: "minimum_input" },
          { kind: "component", component: "matching_text" },
        ],
      }],
    });
    const saved = await saveCompositionGraph(request, seed.spaceId, yaml);
    await page.goto(
      getFrontendUrl(
        `/spaces/${seed.spaceId}/compositions/${saved.composition_id}/${saved.revision_id}/edit`,
      ),
      { waitUntil: "domcontentloaded" },
    );
    const text = page.locator(
      '[data-component-id="matching_text"] :is(p, h2).compositionGraphText',
    );
    await expect(text).toHaveText("Matches: 2", { timeout: 30_000 });
    const minimumInput = page.getByRole("textbox", {
      name: "minimum",
      exact: true,
    });
    await minimumInput.fill("25");
    await expect(text).toHaveText("Matches: 1", { timeout: 30_000 });
    await page.getByRole("button", { name: "Save", exact: true }).click();
    await expect(page).toHaveURL(
      new RegExp(
        `/spaces/${seed.spaceId}/compositions/${saved.composition_id}/[^/]+$`,
      ),
      { timeout: 30_000 },
    );
    await expect(minimumInput).toHaveValue("15");
    await expect(text).toHaveText("Matches: 2", { timeout: 30_000 });
  });

  test("SQL argument edits preserve their explicit variable bindings", async ({ page, request }) => {
    test.setTimeout(120_000);
    const spaceId = await createSpace(request);
    const yaml = await completeCompositionGraphYaml(request, spaceId, {
      name: `Argument editor ${crypto.randomUUID().slice(0, 8)}`,
      variables: [
        {
          id: "threshold",
          kind: "constant",
          value_type: { scalar_type: "integer", nullable: false },
          value: { kind: "integer", value: 3 },
        },
        {
          id: "caption",
          kind: "constant",
          value_type: { scalar_type: "string", nullable: false },
          value: { kind: "string", value: "hello" },
        },
        {
          id: "rows",
          kind: "query",
          returns: {
            kind: "scalar",
            value_type: { scalar_type: "integer", nullable: false },
          },
          query: {
            kind: "inline_sql",
            sql: "SELECT $limit AS value -- $limit stays in a comment",
            arguments: [{ name: "limit", variable: "threshold" }],
            form_bindings: [],
            schema_fingerprint: "",
          },
        },
      ],
      components: [{
        id: "result_text",
        kind: "text",
        content: "Value: {{rows}}",
        style: "body",
      }],
      rows: [{
        id: "results",
        items: [{ kind: "component", component: "result_text" }],
      }],
    });
    const saved = await saveCompositionGraph(request, spaceId, yaml);
    await page.goto(
      getFrontendUrl(
        `/spaces/${spaceId}/compositions/${saved.composition_id}/${saved.revision_id}/edit`,
      ),
      { waitUntil: "domcontentloaded" },
    );
    const text = page.locator(
      '[data-component-id="result_text"] :is(p, h2).compositionGraphText',
    );
    await expect(text).toHaveText("Value: 3", { timeout: 30_000 });
    await page.getByRole("tab", { name: "Variables", exact: true }).click();
    await page.getByRole("button", { name: "rows", exact: true }).click();
    await page.getByRole("button", { name: "Arguments", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Arguments", exact: true });
    await dialog.getByLabel("Argument name for limit", { exact: true })
      .fill("minimum");
    await dialog.getByRole("button", { name: "Rename argument", exact: true })
      .click();
    await expect(
      dialog.getByLabel("Argument name for minimum", { exact: true }),
    )
      .toHaveValue("minimum");
    await expect(
      dialog.getByRole("button", { name: "Remove minimum", exact: true }),
    )
      .toBeDisabled();
    const type = dialog.getByRole("combobox", {
      name: "Type for minimum",
      exact: true,
    });
    await type.selectOption("string");
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
    const binding = page.getByRole("combobox", {
      name: "minimum",
      exact: true,
    });
    await expect(binding).toHaveValue("");
    await expect(page.getByRole("button", { name: "Save", exact: true }))
      .toBeDisabled();
    await binding.selectOption("caption");
    await page.getByRole("button", { name: "Arguments", exact: true }).click();
    await page.getByRole("dialog", { name: "Arguments", exact: true })
      .getByRole("combobox", { name: "Type for minimum", exact: true })
      .selectOption("integer");
    await page.keyboard.press("Escape");
    await expect(binding).toHaveValue("");
    await binding.selectOption("threshold");
    await expect(page.getByRole("button", { name: "Save", exact: true }))
      .toBeEnabled();
    await page.getByRole("button", { name: "Save", exact: true }).click();
    await expect(page).toHaveURL(
      new RegExp(`/compositions/${saved.composition_id}/[^/]+$`),
      { timeout: 30_000 },
    );
    await expect(text).toHaveText("Value: 3", { timeout: 30_000 });
    const path = new URL(page.url()).pathname;
    const revisionId = path.split("/").at(-1)!;
    const revisionResponse = await request.get(
      getBackendUrl(
        `/spaces/${spaceId}/compositions/${saved.composition_id}/history/${revisionId}`,
      ),
    );
    expect(revisionResponse.ok()).toBe(true);
    const revision = await revisionResponse.json() as StudioRawRevision;
    const document = parseYaml(String(revision.fields["spec"])) as {
      spec?: {
        variables?: Array<{
          id?: string;
          query?: {
            kind?: string;
            sql?: string;
            arguments?: Array<{ name: string; variable: string }>;
          };
        }>;
      };
    };
    expect(
      document.spec?.variables?.find((variable) => variable.id === "rows")
        ?.query,
    )
      .toMatchObject({
        kind: "inline_sql",
        sql: "SELECT $minimum AS value -- $limit stays in a comment",
        arguments: [{ name: "minimum", variable: "threshold" }],
      });
  });

  test("Studio modes and the Table picker remain keyboard accessible at narrow widths", async ({ page, request }) => {
    test.setTimeout(120_000);
    const spaceId = await createSpace(request);
    const yaml = await completeCompositionGraphYaml(request, spaceId, {
      name: `Responsive Studio ${crypto.randomUUID().slice(0, 8)}`,
      variables: [{
        id: "rows",
        kind: "query",
        returns: { kind: "table", columns: [] },
        query: {
          kind: "inline_sql",
          sql: "SELECT 1 AS value",
          arguments: [],
          form_bindings: [],
          schema_fingerprint: "",
        },
      }],
      components: [
        { id: "rows_table", kind: "table", variable: "rows" },
        {
          id: "title_text",
          kind: "text",
          content: "Daily report",
          style: "heading",
        },
      ],
      rows: [{
        id: "results",
        items: [
          { kind: "component", component: "title_text" },
          { kind: "component", component: "rows_table" },
        ],
      }],
    });
    const saved = await saveCompositionGraph(request, spaceId, yaml);
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(
      getFrontendUrl(
        `/spaces/${spaceId}/compositions/${saved.composition_id}/${saved.revision_id}/edit`,
      ),
      { waitUntil: "domcontentloaded" },
    );
    const palette = page.getByRole("group", { name: "Add to design" });
    for (const width of [320, 375, 390, 760, 1280]) {
      await page.setViewportSize({ width, height: 844 });
      await page.getByRole("tab", { name: "Design", exact: true }).click();
      await palette.getByRole("button", { name: "Table", exact: true }).click();
      const picker = page.getByRole("dialog", { name: "Table", exact: true });
      await expect(picker.getByRole("button", { name: "rows", exact: true }))
        .toBeVisible();
      await expect(picker.getByRole("tab")).toHaveCount(0);
      await expect(picker.getByText("Saved SQL", { exact: true })).toHaveCount(
        0,
      );
      const widthInViewport = await picker.evaluate((element) =>
        element.getBoundingClientRect().width
      );
      expect(widthInViewport).toBeLessThanOrEqual(width - 32);
      await page.keyboard.press("Escape");
      await expect(picker).toHaveCount(0);
      await expect(palette.getByRole("button", { name: "Table", exact: true }))
        .toBeFocused();

      for (const mode of ["Design", "Variables"] as const) {
        const option = page.getByRole("tab", { name: mode, exact: true });
        await option.click();
        await option.focus();
        await page.keyboard.press("Tab");
        await page.keyboard.press("Shift+Tab");
        const focusStyle = await option.evaluate((element) => {
          const style = getComputedStyle(element);
          return {
            visible: element.matches(":focus-visible"),
            outlineStyle: style.outlineStyle,
            outlineWidth: Number.parseFloat(style.outlineWidth),
          };
        });
        expect(focusStyle.visible).toBe(true);
        expect(["auto", "solid"]).toContain(focusStyle.outlineStyle);
        expect(focusStyle.outlineWidth).toBeGreaterThanOrEqual(2);
        const dimensions = await page.evaluate(() => ({
          client: document.documentElement.clientWidth,
          content: document.documentElement.scrollWidth,
        }));
        expect(dimensions.content).toBeLessThanOrEqual(dimensions.client);
      }
    }
    await page.getByRole("tab", { name: "Variables", exact: true }).click();
    await page.getByRole("button", { name: "Add variable", exact: true })
      .click();
    const addMenu = page.getByRole("group", { name: "Add variable" });
    await expect(addMenu.getByRole("button", { name: "SQL", exact: true }))
      .toBeVisible();
    await expect(addMenu.getByText("Saved SQL", { exact: true })).toHaveCount(
      0,
    );
    await expect(addMenu.getByText("Metric", { exact: true })).toHaveCount(0);
  });
});
