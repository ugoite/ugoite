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
  saveCompositionGraph,
} from "./lib/composition-graph.ts";

type StudioSeedState = {
  spaceId: string;
  formName: string;
  savedSqlId: string;
  savedSqlRevisionId: string;
  savedSqlName: string;
  toolName: string;
  monthStart: string;
  monthEnd: string;
  expectedTotal: string;
  expectedQuantity: string;
  purposes: string[];
};

type ReactiveMetricSeed = {
  spaceId: string;
  formName: string;
  savedSqlId: string;
  savedSqlRevisionId: string;
  savedSqlName: string;
  toolName: string;
};

type StudioHistoryPage = {
  revisions: Array<{ revision: { revision_id: string } }>;
  total: number;
};

type StudioRawRevision = {
  revision: { entry_id: string; revision_id: string };
  fields: Record<string, unknown>;
};

function savedSqlVariablePrefix(name: string): string {
  const slug = name.normalize("NFKD").toLowerCase().replace(/\p{M}/gu, "")
    .replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "");
  return slug && /^[a-z]/.test(slug) ? slug : slug ? `query_${slug}` : "query";
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
  const savedSqlName = `Studio monthly summary ${suffix}`;
  const toolName = `Studio monthly tool ${suffix}`;
  const monthStart = "2026-01-01";
  const monthEnd = "2026-02-01";
  // Exactly one January row: metric displays need one exact-scalar row. The
  // result exposes two independent columns so multiple Metrics can share it.
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
    fields?: Record<string, { sql_column?: string }>;
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

  // Plain column selects with ORDER BY keep this fixture deterministic while
  // exercising Saved SQL variable binding; the January window matches one
  // row so both metrics render scalars instead of the multiple-rows diagnostic.
  const sql = `SELECT ${sqlIdentifier(amountColumn!)} AS total, ${
    sqlIdentifier(quantityColumn!)
  } AS quantity FROM ${sqlIdentifier(form!.sql_relation!)} WHERE ${
    sqlIdentifier(dateColumn!)
  } >= $month_start AND ${sqlIdentifier(dateColumn!)} < $month_end ORDER BY ${
    sqlIdentifier(dateColumn!)
  } ASC`;
  const sqlResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name: savedSqlName,
        kind: "user-query",
        sql,
        variables: [
          {
            type: "date",
            name: "month_start",
            description: "First date included in this month.",
          },
          {
            type: "date",
            name: "month_end",
            description: "First date excluded from this month.",
          },
        ],
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

  return {
    spaceId,
    formName,
    savedSqlId: savedSql.id!,
    savedSqlRevisionId: savedSql.revision_id!,
    savedSqlName,
    toolName,
    monthStart,
    monthEnd,
    expectedTotal: "20",
    expectedQuantity: "3",
    purposes,
  };
}

async function seedReactiveMetricKnowledge(
  request: APIRequestContext,
): Promise<ReactiveMetricSeed> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `FilteredExpense${suffix}`;
  const savedSqlName = `Filtered expense totals ${suffix}`;
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
    name?: string;
    sql_relation?: string;
    fields?: Record<string, { sql_column?: string }>;
  }>;
  const form = forms.find((candidate) => candidate.name === formName);
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
  const sqlResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name: savedSqlName,
        kind: "user-query",
        sql:
          `SELECT COUNT(*) AS matching_count, SUM(${amount}) AS total_amount FROM ${
            sqlIdentifier(form!.sql_relation!)
          } WHERE ${amount} >= $minimum`,
        variables: [{
          type: "integer",
          name: "minimum",
          description: "Minimum expense amount included.",
        }],
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

  return {
    spaceId,
    formName,
    savedSqlId: savedSql.id!,
    savedSqlRevisionId: savedSql.revision_id!,
    savedSqlName,
    toolName,
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

async function addCanvasDisplay(
  page: Page,
  kind: "Metric" | "Table",
  sourceName: string,
  column?: string,
  arguments_: Record<string, string> = {},
): Promise<void> {
  const mode = page.getByRole("tablist", { name: "Studio mode" });
  await mode.getByRole("tab", { name: "Design", exact: true }).click();
  await page.getByRole("button", { name: "Add to design", exact: true })
    .first().click();
  const palette = page.getByRole("group", { name: "Add to design" });
  await palette.getByRole("button", { name: kind, exact: true }).click();
  const dialog = page.getByRole("dialog", { name: kind, exact: true });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("tab", { name: "Saved SQL", exact: true }).click();
  const savedSql = dialog.getByRole("button", {
    name: sourceName,
    exact: true,
  });
  await expect(savedSql).toBeEnabled({ timeout: 30_000 });
  await savedSql.click();
  for (const [argument, variable] of Object.entries(arguments_)) {
    await dialog.getByLabel(`Variable for SQL argument ${argument}`, {
      exact: true,
    }).selectOption({ label: variable });
  }
  if (Object.keys(arguments_).length > 0) {
    await dialog.getByRole("button", { name: "Add", exact: true }).click();
  }
  if (kind === "Metric") {
    if (column) {
      const resultColumn = dialog.getByRole("button", {
        name: column,
        exact: true,
      });
      await expect(resultColumn).toBeVisible({ timeout: 30_000 });
      await resultColumn.click();
    }
  }
  await expect(dialog).toHaveCount(0);
}

async function addMetricDisplay(
  page: Page,
  sourceName: string,
  column: string,
  arguments_: Record<string, string> = {},
): Promise<void> {
  await addCanvasDisplay(page, "Metric", sourceName, column, arguments_);
}

async function addStudioTypedInput(
  page: Page,
  variableId: string,
  type: string,
  defaultValue: string,
): Promise<void> {
  await page.getByRole("tablist", { name: "Studio mode" })
    .getByRole("tab", { name: "Variables", exact: true }).click();
  await page.getByRole("button", { name: "Add variable", exact: true }).click();
  await page.getByRole("group", { name: "Add variable" })
    .getByRole("button", { name: "Input", exact: true }).click();
  await page.getByLabel("Variable ID", { exact: true }).fill(variableId);
  await page.getByLabel("Type", { exact: true }).selectOption(type);
  if (defaultValue) {
    await page.getByLabel("Default", { exact: true }).fill(defaultValue);
  }
  await page.getByRole("tablist", { name: "Studio mode" })
    .getByRole("tab", { name: "Design", exact: true }).click();
}

async function placeStudioInput(
  page: Page,
  variableId: string,
): Promise<void> {
  await page.getByRole("tablist", { name: "Studio mode" })
    .getByRole("tab", { name: "Design", exact: true }).click();
  await page.getByRole("button", { name: "Add to design", exact: true })
    .first().click();
  await page.getByRole("group", { name: "Add to design" })
    .getByLabel("Input", { exact: true }).selectOption({ label: variableId });
}

async function addStudioText(page: Page, variableId: string): Promise<void> {
  await page.getByRole("tablist", { name: "Studio mode" })
    .getByRole("tab", { name: "Design", exact: true }).click();
  await page.getByRole("button", { name: "Add to design", exact: true })
    .first().click();
  await page.getByRole("group", { name: "Add to design" })
    .getByRole("button", { name: "Text", exact: true }).click();
  const textBlock = page.getByRole("button", {
    name: "text, Text",
    exact: true,
  });
  await expect(textBlock).toBeVisible();
  await textBlock.click();
  await page.getByRole("complementary", { name: "Selected component" })
    .getByLabel("Variables", { exact: true })
    .selectOption({ label: variableId });
}

async function addStudioInput(
  page: Page,
  variableId: string,
): Promise<void> {
  await placeStudioInput(page, variableId);
}

function graphSavedSqlQuery(
  id: string,
  savedSqlId: string,
  savedSqlRevisionId: string,
  column: string | undefined,
  returns: Record<string, unknown>,
  args: Array<{ name: string; variable: string }>,
) {
  return {
    id,
    kind: "query" as const,
    returns,
    query: {
      kind: "saved_sql",
      entry_id: savedSqlId,
      revision_id: savedSqlRevisionId,
      ...(column ? { column } : {}),
      arguments: args,
      form_bindings: [],
      schema_fingerprint: "",
    },
  };
}

function buildMobileStudioSeed(
  compositionName: string,
  savedSqlId: string,
  savedSqlRevisionId: string,
  monthStart: string,
  monthEnd: string,
) {
  const args = [
    { name: "month_start", variable: "month_start" },
    { name: "month_end", variable: "month_end" },
  ];
  return {
    name: compositionName,
    variables: [
      {
        id: "month_start",
        kind: "input" as const,
        value_type: { scalar_type: "date", nullable: false },
        required: true,
        default: { kind: "date", value: monthStart },
      },
      {
        id: "month_end",
        kind: "input" as const,
        value_type: { scalar_type: "date", nullable: false },
        required: true,
        default: { kind: "date", value: monthEnd },
      },
      graphSavedSqlQuery(
        "studio_rows",
        savedSqlId,
        savedSqlRevisionId,
        undefined,
        { kind: "table", columns: [] },
        args,
      ),
      graphSavedSqlQuery(
        "studio_total",
        savedSqlId,
        savedSqlRevisionId,
        "total",
        {
          kind: "scalar",
          value_type: { scalar_type: "float", nullable: false },
        },
        args,
      ),
    ],
    components: [
      { id: "month_start_input", kind: "input", variable: "month_start" },
      { id: "month_end_input", kind: "input", variable: "month_end" },
      { id: "total", kind: "metric", variable: "studio_total" },
      { id: "rows", kind: "table", variable: "studio_rows" },
    ],
    rows: [{
      id: "main",
      items: [
        { kind: "component" as const, component: "month_start_input" },
        { kind: "component" as const, component: "month_end_input" },
        { kind: "component" as const, component: "total" },
        { kind: "component" as const, component: "rows" },
      ],
    }],
  };
}

async function openToolFromSavedTools(
  page: Page,
  spaceId: string,
  compositionId: string,
  revisionId: string,
  toolName: string,
): Promise<string> {
  const path = `/spaces/${spaceId}/compositions/${compositionId}/${revisionId}`;
  await page.goto(getFrontendUrl(`/spaces/${spaceId}/compositions`), {
    waitUntil: "domcontentloaded",
  });
  const link = page.locator(`a[href="${path}"]`);
  await expect(link).toBeVisible();
  await link.click();
  await expect(page).toHaveURL(new RegExp(`${path.replaceAll("/", "\\/")}$`));
  await expect(
    page.getByRole("heading", { level: 1, name: toolName }),
  ).toBeVisible();
  return path;
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

  test("Composition Studio builds a reusable tool from existing knowledge", async ({ browser, context, request }) => {
    test.setTimeout(120_000);
    const storageState = await context.storageState();
    let compositionId = "";
    let firstRevisionId = "";
    let editedRevisionId = "";

    const buildContext = await openOwnerContext(browser, storageState);
    try {
      const page = await buildContext.newPage();
      await page.goto(
        getFrontendUrl(
          `/spaces/${seed.spaceId}/sql/${seed.savedSqlId}/variables`,
        ),
        { waitUntil: "domcontentloaded" },
      );
      await page.getByLabel("month_start").fill(seed.monthStart);
      await page.getByLabel("month_end").fill(seed.monthEnd);
      await page.getByRole("button", { name: "Run", exact: true }).click();
      await expect(page).toHaveURL(
        new RegExp(
          `/spaces/${seed.spaceId}/sql/${seed.savedSqlId}/run$`,
        ),
        { timeout: 30_000 },
      );
      const runTable = page.getByRole("table");
      await expect(runTable.locator("tbody tr")).toHaveCount(1);
      await expect(runTable).toContainText(seed.expectedTotal, {
        timeout: 30_000,
      });

      // Save-as-tool pins the exact Saved SQL revision in the binding picker.
      await page.getByRole("button", { name: "Save as tool" }).click();
      await expect(page).toHaveURL(
        new RegExp(
          `/spaces/${seed.spaceId}/compositions/new(?:\\?.*)?$`,
        ),
        { timeout: 30_000 },
      );
      const nameInput = page.getByLabel("Name", { exact: true });
      await expect(nameInput).toHaveValue(seed.savedSqlName);
      const initialPicker = page.getByRole("dialog", { name: "Table" });
      await expect(initialPicker).toBeVisible();
      await page.keyboard.press("Escape");
      await expect(initialPicker).toHaveCount(0);
      await nameInput.fill(seed.toolName);
      await addStudioTypedInput(page, "month_start", "date", seed.monthStart);
      await addStudioTypedInput(page, "month_end", "date", seed.monthEnd);
      await addStudioTypedInput(
        page,
        "report_title",
        "string",
        "January expenses",
      );

      // Inputs are explicit graph components; query arguments are explicitly
      // bound to the input variables in the same Saved SQL picker.
      await addStudioInput(page, "month_start");
      await addStudioInput(page, "month_end");
      await addStudioInput(page, "report_title");
      await addStudioText(page, "report_title");
      const args = { month_start: "month_start", month_end: "month_end" };
      await addCanvasDisplay(page, "Table", seed.savedSqlName, undefined, args);
      await addMetricDisplay(page, seed.savedSqlName, "total", args);
      await addMetricDisplay(page, seed.savedSqlName, "quantity", args);
      const canvasBlocks = page.locator(
        ".compositionGraphCanvas .compositionGraphDesignValue",
      );
      await expect(canvasBlocks).toHaveCount(7);

      // Save once: exactly one publication.
      await page.getByRole("button", { name: "Save", exact: true }).click();
      await expect(page).toHaveURL(
        new RegExp(
          `/spaces/${seed.spaceId}/compositions/[^/]+/[^/]+$`,
        ),
        { timeout: 30_000 },
      );
      const saved = revisionFromUrl(page.url());
      compositionId = saved.compositionId;
      firstRevisionId = saved.revisionId;
      await expectHistoryTotal(request, seed.spaceId, compositionId, 1);

      const exactResponse = await request.get(
        getBackendUrl(
          `/spaces/${seed.spaceId}/compositions/${compositionId}/history/${firstRevisionId}`,
        ),
      );
      expect(exactResponse.ok()).toBe(true);
      const exactRevision = await exactResponse.json() as StudioRawRevision;
      expect(exactRevision.revision.revision_id).toBe(firstRevisionId);
      const savedSpec = exactRevision.fields["spec"];
      expect(typeof savedSpec).toBe("string");
      const savedYaml = String(savedSpec);
      expect(savedYaml).toContain(`revision_id: ${seed.savedSqlRevisionId}`);
      expect(savedYaml).toMatch(/^\s+variable: month_start\s*$/m);
      expect(savedYaml).toMatch(/^\s+variable: month_end\s*$/m);
      expect(savedYaml).not.toMatch(/\bsource_id\b|\bsource:/);
    } finally {
      await buildContext.close();
    }

    const journeyContext = await openOwnerContext(browser, storageState);
    try {
      const page = await journeyContext.newPage();
      const evaluatePayloads: Array<Record<string, unknown>> = [];
      page.on("request", (request) => {
        const pathname = new URL(request.url()).pathname;
        if (pathname.endsWith("/compositions/evaluate")) {
          try {
            evaluatePayloads.push(
              request.postDataJSON() as Record<string, unknown>,
            );
          } catch {
            // The request count remains useful if the transport omits JSON.
            evaluatePayloads.push({});
          }
        }
      });
      // The Saved tools list rediscovers the exact saved revision.
      await openToolFromSavedTools(
        page,
        seed.spaceId,
        compositionId,
        firstRevisionId,
        seed.toolName,
      );
      await expect(page.getByLabel("month_start")).toHaveValue(
        seed.monthStart,
      );
      await expect(page.getByLabel("report_title", { exact: true }))
        .toHaveValue("January expenses", { timeout: 30_000 });
      await expect(page.getByText("January expenses", { exact: true }))
        .toBeVisible();
      await expect(page.getByLabel("month_start")).toBeEditable();
      await expect(page.getByLabel("month_end")).toHaveValue(seed.monthEnd);
      await expect(page.getByLabel("month_end")).toBeEditable();
      const metrics = page.locator(".compositionGraphMetric strong");
      await expect(metrics).toHaveCount(2, { timeout: 30_000 });
      await expect(metrics).toContainText([
        seed.expectedTotal,
        seed.expectedQuantity,
      ]);
      const savedTable = page.locator(".compositionGraphTableViewport table");
      await expect(savedTable).toContainText(seed.expectedTotal, {
        timeout: 30_000,
      });
      await expect(savedTable).toContainText(seed.expectedQuantity, {
        timeout: 30_000,
      });

      // A Text-only input update does not recompute a query variable.
      await expect.poll(() => evaluatePayloads.length).toBeGreaterThan(0);
      const initialEvaluateCount = evaluatePayloads.length;
      await page.getByLabel("report_title", { exact: true }).fill(
        "Transient report title",
      );
      await expect(
        page.getByText("Transient report title", { exact: true }),
      ).toBeVisible();
      await expect.poll(() => evaluatePayloads.length).toBeGreaterThan(
        initialEvaluateCount,
      );
      expect(evaluatePayloads.at(-1)?.requested_queries).toEqual([]);

      // Runtime edits live only in Work: a fresh load of the saved revision
      // restores the declared default and does not publish another revision.
      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(page.getByLabel("report_title")).toHaveValue(
        "January expenses",
        { timeout: 30_000 },
      );
      await expect(page.getByText("January expenses", { exact: true }))
        .toBeVisible({ timeout: 30_000 });
      await expectHistoryTotal(request, seed.spaceId, compositionId, 1);

      // Edit the exact revision by changing a component's variable reference.
      await page.goto(
        getFrontendUrl(
          `/spaces/${seed.spaceId}/compositions/${compositionId}/${firstRevisionId}/edit`,
        ),
        { waitUntil: "domcontentloaded" },
      );
      // The edit studio loads the exact revision asynchronously; wait for
      // the canvas before asserting the restored name.
      await expect(
        page.getByRole("button", { name: "Add to design", exact: true })
          .first(),
      ).toBeVisible({ timeout: 30_000 });
      await expect(page.getByLabel("Name", { exact: true })).toHaveValue(
        seed.toolName,
      );
      const prefix = savedSqlVariablePrefix(seed.savedSqlName);
      const totalVariable = `${prefix}_total`;
      const quantityVariable = `${prefix}_quantity`;
      await page.getByRole("button", {
        name: `metric, ${totalVariable}`,
        exact: true,
      }).click();
      await page.getByRole("button", {
        name: `Change variable, ${totalVariable}`,
        exact: true,
      }).click();
      const metricPicker = page.getByRole("dialog", { name: "Metric" });
      await metricPicker.getByRole("tab", { name: "Variables", exact: true })
        .click();
      await metricPicker.getByRole("button", {
        name: quantityVariable,
        exact: true,
      }).click();
      await expect(metricPicker).toHaveCount(0);
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
      await expectHistoryTotal(request, seed.spaceId, compositionId, 2);
      await expect(page.locator(".compositionGraphMetric strong"))
        .toHaveCount(2, { timeout: 30_000 });
      await expect(page.getByLabel("month_start")).toHaveValue(seed.monthStart);
      await expect(page.getByLabel("month_end")).toHaveValue(seed.monthEnd);

      // History lists both revisions; restoring the first appends a third.
      await page.goto(
        getFrontendUrl(
          `/spaces/${seed.spaceId}/compositions/${compositionId}/history`,
        ),
        { waitUntil: "domcontentloaded" },
      );
      const createdLink = page.getByRole("link", { name: /^Created/ });
      await expect(createdLink).toBeVisible();
      await expect(
        page.getByRole("link", { name: /^Edited/ }),
      ).toBeVisible();
      await createdLink.click();
      await expect(page).toHaveURL(
        new RegExp(
          `/spaces/${seed.spaceId}/compositions/${compositionId}/${firstRevisionId}$`,
        ),
        { timeout: 30_000 },
      );
      await expect(
        page.getByRole("heading", { name: "total", exact: true }),
      ).toBeVisible({ timeout: 30_000 });
      await page.getByRole("button", {
        name: "Restore this revision",
        exact: true,
      }).click();
      // Restore appends a revision and navigates to it; the URL shape is
      // unchanged, so wait for the URL itself to move before reading it.
      const preRestoreUrl = page.url();
      await expect
        .poll(async () => page.url(), { timeout: 30_000 })
        .not.toBe(preRestoreUrl);
      await expect(page).toHaveURL(
        new RegExp(
          `/spaces/${seed.spaceId}/compositions/${compositionId}/[^/]+$`,
        ),
        { timeout: 30_000 },
      );
      const restored = revisionFromUrl(page.url());
      expect(restored.compositionId).toBe(compositionId);
      expect(restored.revisionId).not.toBe(firstRevisionId);
      expect(restored.revisionId).not.toBe(editedRevisionId);
      await expectHistoryTotal(request, seed.spaceId, compositionId, 3);
      await expect(
        page.getByRole("heading", { name: "total", exact: true }),
      ).toBeVisible({ timeout: 30_000 });
      await expect(
        page.getByText("Total spent", { exact: true }),
      ).toHaveCount(0);
    } finally {
      await journeyContext.close();
    }
  });

  test("Form bindings create variable-backed displays with explicit aggregation", async ({ browser, context, request }) => {
    const storageState = await context.storageState();
    const ownerContext = await openOwnerContext(browser, storageState);
    const page = await ownerContext.newPage();
    try {
      const formsResponse = await request.get(
        getBackendUrl(`/spaces/${seed.spaceId}/forms`),
      );
      expect(formsResponse.ok()).toBe(true);
      const forms = await formsResponse.json() as Array<{
        id?: string;
        name?: string;
        sql_relation?: string;
        fields?: Record<string, { sql_column?: string }>;
      }>;
      const form = forms.find((candidate) => candidate.name === seed.formName);
      expect(form?.id).toBeTruthy();
      expect(form?.sql_relation).toBeTruthy();
      const amountColumn = form?.fields?.Amount?.sql_column;
      expect(amountColumn).toBeTruthy();
      const formSlug = seed.formName.normalize("NFKD").toLowerCase()
        .replace(/\p{M}/gu, "")
        .replace(/[^a-z0-9]+/g, "_")
        .replace(/^_+|_+$/g, "");

      await page.goto(
        getFrontendUrl(`/spaces/${seed.spaceId}/compositions/new`),
        { waitUntil: "domcontentloaded" },
      );
      await page.getByLabel("Name", { exact: true }).fill(
        `${seed.toolName} Form bindings`,
      );

      await page.getByRole("button", { name: "Add to design", exact: true })
        .first().click();
      const palette = page.getByRole("group", { name: "Add to design" });
      await palette.getByRole("button", { name: "Table", exact: true }).click();
      const tablePicker = page.getByRole("dialog", {
        name: "Table",
        exact: true,
      });
      await tablePicker.getByRole("tab", { name: "Forms", exact: true })
        .click();
      await tablePicker.getByRole("button", {
        name: seed.formName,
        exact: true,
      }).click();
      await expect(tablePicker).toHaveCount(0);
      await expect(page.getByRole("button", { name: /Change variable/ }))
        .toHaveCount(0);

      await page.getByRole("button", { name: "Add to design", exact: true })
        .first().click();
      await page.getByRole("group", { name: "Add to design" })
        .getByRole("button", { name: "Metric", exact: true }).click();
      const metricPicker = page.getByRole("dialog", {
        name: "Metric",
        exact: true,
      });
      await metricPicker.getByRole("tab", { name: "Forms", exact: true })
        .click();
      await metricPicker.getByRole("button", {
        name: seed.formName,
        exact: true,
      }).click();
      const aggregation = metricPicker.getByRole("combobox", {
        name: "Aggregation",
        exact: true,
      });
      await expect(aggregation).toHaveValue("");
      const addMetric = metricPicker.getByRole("button", {
        name: "Add",
        exact: true,
      });
      await expect(addMetric).toBeDisabled();
      await aggregation.selectOption("sum");
      await metricPicker.getByRole("combobox", {
        name: "Column",
        exact: true,
      }).selectOption({ label: "Amount" });
      await addMetric.click();
      await expect(metricPicker).toHaveCount(0);
      await expect(page.getByRole("button", { name: /Change variable/ }))
        .toHaveCount(0);

      const designTable = page.getByRole("table", {
        name: `${formSlug}_rows results`,
      });
      await expect(designTable).toContainText(seed.purposes[0]);
      await expect(designTable).toContainText(seed.purposes[1]);
      await expect(page.getByLabel(`${formSlug}_sum_amount`, { exact: true }))
        .toContainText("50");
      await page.getByRole("button", {
        name: `metric, ${formSlug}_sum_amount`,
        exact: true,
      }).click();
      const moveEarlier = page.getByRole("button", {
        name: "Move earlier",
        exact: true,
      });
      await moveEarlier.focus();
      await page.keyboard.press("Enter");
      await expect(page.locator(".compositionGraphDesignValue").first())
        .toContainText("50");
      for (const width of [320, 375, 390]) {
        await page.setViewportSize({ width, height: 844 });
        expect(
          await page.evaluate(() =>
            document.documentElement.scrollWidth <= window.innerWidth
          ),
        ).toBe(true);
        await expect(designTable).toContainText(seed.purposes[0]);
      }
      await test.info().attach("Composition live Design at 390px", {
        body: await page.screenshot({ fullPage: true }),
        contentType: "image/png",
      });
      await page.getByRole("button", { name: "Save", exact: true }).click();
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${seed.spaceId}/compositions/[^/]+/[^/]+$`),
        { timeout: 30_000 },
      );
      await expect(
        page.getByRole("table", { name: `${formSlug}_rows results` }),
      ).toContainText(seed.purposes[0]);
      await expect(page.getByLabel(`${formSlug}_sum_amount`, { exact: true }))
        .toContainText("50");
      const saved = revisionFromUrl(page.url());
      const response = await request.get(
        getBackendUrl(
          `/spaces/${seed.spaceId}/compositions/${saved.compositionId}/history/${saved.revisionId}`,
        ),
      );
      expect(response.ok()).toBe(true);
      const revision = await response.json() as StudioRawRevision;
      const savedYaml = String(revision.fields["spec"]);
      const components =
        savedYaml.split("components:")[1]?.split("layout:")[0] ??
          "";

      expect(savedYaml).toContain(
        `SELECT * FROM "${form?.sql_relation}"`,
      );
      expect(savedYaml).toContain(
        `SUM("${amountColumn}") AS value FROM "${form?.sql_relation}"`,
      );
      expect(components).toContain(`variable: ${formSlug}_rows`);
      expect(components).toContain(
        `variable: ${formSlug}_sum_amount`,
      );
      expect(components).not.toMatch(/\b(?:form_id|source_id|source):/);
    } finally {
      await ownerContext.close();
    }
  });

  test("Composition Studio saves and reopens a filtered SQL metric", async ({ browser, context, request }) => {
    test.setTimeout(120_000);
    const seed = await seedReactiveMetricKnowledge(request);
    const storageState = await context.storageState();
    let compositionId = "";
    let revisionId = "";

    const buildContext = await openOwnerContext(browser, storageState);
    try {
      const page = await buildContext.newPage();
      await page.goto(
        getFrontendUrl(
          `/spaces/${seed.spaceId}/sql/${seed.savedSqlId}/variables`,
        ),
        { waitUntil: "domcontentloaded" },
      );
      const minimumInput = page.getByRole("textbox", {
        name: "minimum integer",
        exact: true,
      });
      await expect(minimumInput).toBeVisible();
      await minimumInput.fill("0");
      await page.getByRole("button", { name: "Run", exact: true }).click();
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${seed.spaceId}/sql/${seed.savedSqlId}/run$`),
        { timeout: 30_000 },
      );
      const sqlResult = page.getByRole("table");
      await expect(sqlResult.locator("tbody tr")).toHaveCount(1);
      await expect(sqlResult).toContainText("3", { timeout: 30_000 });
      await expect(sqlResult).toContainText("60", { timeout: 30_000 });

      await page.getByRole("button", { name: "Save as tool" }).click();
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${seed.spaceId}/compositions/new(?:\\?.*)?$`),
        { timeout: 30_000 },
      );
      const initialPicker = page.getByRole("dialog", { name: "Table" });
      await expect(initialPicker).toBeVisible();
      await page.keyboard.press("Escape");
      await expect(initialPicker).toHaveCount(0);
      await page.getByLabel("Name", { exact: true }).fill(seed.toolName);
      await addStudioTypedInput(page, "minimum", "integer", "0");
      await addStudioInput(page, "minimum");

      await addMetricDisplay(
        page,
        seed.savedSqlName,
        "matching_count",
        { minimum: "minimum" },
      );
      await addMetricDisplay(
        page,
        seed.savedSqlName,
        "total_amount",
        { minimum: "minimum" },
      );
      await page.getByRole("button", { name: "Save", exact: true }).click();
      await expect(page).toHaveURL(
        new RegExp(
          `/spaces/${seed.spaceId}/compositions/[^/]+/[^/]+$`,
        ),
        { timeout: 30_000 },
      );
      const saved = revisionFromUrl(page.url());
      compositionId = saved.compositionId;
      revisionId = saved.revisionId;
      await expectHistoryTotal(request, seed.spaceId, compositionId, 1);

      const exactResponse = await request.get(
        getBackendUrl(
          `/spaces/${seed.spaceId}/compositions/${compositionId}/history/${revisionId}`,
        ),
      );
      expect(exactResponse.ok()).toBe(true);
      const exactRevision = await exactResponse.json() as StudioRawRevision;
      expect(exactRevision.revision.revision_id).toBe(revisionId);
      expect(String(exactRevision.fields["spec"])).toContain(
        `revision_id: ${seed.savedSqlRevisionId}`,
      );
      expect(String(exactRevision.fields["spec"])).not.toMatch(
        /\bsource_id\b|\bsource:/,
      );
    } finally {
      await buildContext.close();
    }

    const journeyContext = await openOwnerContext(browser, storageState);
    try {
      const page = await journeyContext.newPage();
      const evaluatePayloads: Array<Record<string, unknown>> = [];
      page.on("request", (request) => {
        if (
          new URL(request.url()).pathname.endsWith("/compositions/evaluate")
        ) {
          try {
            evaluatePayloads.push(
              request.postDataJSON() as Record<string, unknown>,
            );
          } catch {
            evaluatePayloads.push({});
          }
        }
      });

      await openToolFromSavedTools(
        page,
        seed.spaceId,
        compositionId,
        revisionId,
        seed.toolName,
      );
      const metrics = page.locator(".compositionGraphMetric strong");
      const metricValues = () =>
        metrics.allTextContents().then((values) =>
          values.sort((left, right) => Number(left) - Number(right))
        );

      const minimumInput = page.getByLabel("minimum", { exact: true });
      await expect(minimumInput).toBeVisible({ timeout: 30_000 });
      await expect(minimumInput).toHaveValue("0");
      await expect.poll(metricValues, { timeout: 30_000 }).toEqual([
        "3",
        "60",
      ]);
      const initialQueryCount = evaluatePayloads.length;
      expect(initialQueryCount).toBeGreaterThan(0);

      await page.getByLabel("minimum", { exact: true }).fill("20");
      await expect.poll(metricValues, { timeout: 30_000 }).toEqual([
        "2",
        "50",
      ]);
      await expect.poll(() => evaluatePayloads.length).toBeGreaterThan(
        initialQueryCount,
      );
      expect(evaluatePayloads.at(-1)?.requested_queries).toEqual(
        expect.arrayContaining([
          `${savedSqlVariablePrefix(seed.savedSqlName)}_matching_count`,
          `${savedSqlVariablePrefix(seed.savedSqlName)}_total_amount`,
        ]),
      );

      // Input state is transient: reload returns to the saved default and does
      // not append a Composition revision.
      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(page.getByLabel("minimum", { exact: true }))
        .toHaveValue("0", { timeout: 30_000 });
      await expect.poll(metricValues, { timeout: 30_000 }).toEqual([
        "3",
        "60",
      ]);
      await expectHistoryTotal(request, seed.spaceId, compositionId, 1);
    } finally {
      await journeyContext.close();
    }
  });

  test("Pinned Saved SQL source supports an explicit inline copy and explicit counts", async ({ page, request }) => {
    test.setTimeout(120_000);
    const suffix = crypto.randomUUID().slice(0, 8);
    const spaceId = seed.spaceId;
    const sqlName = `Pinned SQL ${suffix}`;
    const created = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: {
          name: sqlName,
          kind: "user-query",
          sql: "SELECT CAST(11 AS BIGINT) AS pinned_value",
          variables: [],
        },
      },
    );
    expect([200, 201]).toContain(created.status());
    const savedSql = await created.json() as {
      id: string;
      revision_id: string;
    };
    await page.goto(getFrontendUrl(`/spaces/${spaceId}/compositions/new`));
    await page.getByLabel("Name", { exact: true }).fill(
      `Pinned tool ${suffix}`,
    );
    await addCanvasDisplay(page, "Table", sqlName);
    const savePath = `/spaces/${spaceId}/compositions`;
    const firstSave = page.waitForResponse((response) =>
      new URL(response.url()).pathname.endsWith(savePath) &&
      response.request().method() === "POST"
    );
    await page.getByRole("button", { name: "Save", exact: true }).click();
    const savedResponse = await firstSave;
    expect(savedResponse.status()).toBe(201);
    const saved = await savedResponse.json() as {
      composition_id: string;
      revision_id: string;
    };
    await expect(page).toHaveURL(
      new RegExp(`${savePath}/${saved.composition_id}/${saved.revision_id}$`),
      { timeout: 30_000 },
    );
    const updated = await request.put(
      getBackendUrl(`/spaces/${spaceId}/sql/${savedSql.id}`),
      {
        data: {
          name: sqlName,
          kind: "user-query",
          sql: "SELECT CAST(99 AS BIGINT) AS pinned_value",
          variables: [],
          parent_revision_id: savedSql.revision_id,
        },
      },
    );
    expect(updated.ok()).toBe(true);
    const latest = await updated.json() as { revision_id: string };
    await page.goto(
      getFrontendUrl(
        `${savePath}/${saved.composition_id}/${saved.revision_id}/edit`,
      ),
    );
    await page.getByRole("tab", { name: "Variables", exact: true }).click();
    await page.locator(
      ".compositionVariableRail--desktop .compositionVariableRailItem",
    ).first().click();
    const editor = page.getByRole("textbox", { name: "SQL", exact: true });
    await expect(editor).toContainText("11", { timeout: 30_000 });
    await expect(editor).not.toContainText("99");
    await expect(editor).toHaveAttribute("contenteditable", "false");
    const counts: Array<Record<string, unknown>> = [];
    page.on("request", (outgoing) => {
      if (new URL(outgoing.url()).pathname.endsWith("/sql/query/count")) {
        counts.push(outgoing.postDataJSON());
      }
    });
    await page.getByRole("button", { name: "Run Query", exact: true }).click();
    await expect(page.getByRole("table")).toContainText("11", {
      timeout: 30_000,
    });
    expect(counts).toHaveLength(0);
    const countResponse = page.waitForResponse((response) =>
      new URL(response.url()).pathname.endsWith("/sql/query/count")
    );
    await page.getByRole("button", { name: "Count rows", exact: true }).click();
    expect(await (await countResponse).json()).toMatchObject({ count: 1 });
    expect(counts[0].saved_sql).toEqual({
      id: savedSql.id,
      revision_id: savedSql.revision_id,
    });
    await page.getByRole("button", { name: "Edit an inline copy", exact: true })
      .click();
    await expect(editor).toHaveAttribute("contenteditable", "true");
    await expect(editor).toContainText("11");
    await page.getByRole("button", { name: "Run Query", exact: true }).click();
    await expect(page.getByRole("table")).toContainText("11", {
      timeout: 30_000,
    });
    expect(counts).toHaveLength(1);
    const inlineCount = page.waitForResponse((response) =>
      new URL(response.url()).pathname.endsWith("/sql/query/count")
    );
    await page.getByRole("button", { name: "Count rows", exact: true }).click();
    expect(await (await inlineCount).json()).toMatchObject({ count: 1 });
    expect(counts[1]).not.toHaveProperty("saved_sql");
    expect(counts[1].sql).toContain("11");
    await page.setViewportSize({ width: 390, height: 844 });
    expect(
      await page.evaluate(() =>
        document.documentElement.scrollWidth <= window.innerWidth
      ),
    ).toBe(true);
    const screenshot = test.info().outputPath(
      "composition-inline-copy-390.png",
    );
    await page.screenshot({ path: screenshot, fullPage: true });
    await test.info().attach("Composition inline copy at 390px", {
      path: screenshot,
      contentType: "image/png",
    });
    const secondSave = page.waitForResponse((response) =>
      new URL(response.url()).pathname.endsWith(savePath) &&
      response.request().method() === "POST"
    );
    await page.getByRole("button", { name: "Save", exact: true }).click();
    const secondResponse = await secondSave;
    expect(secondResponse.ok()).toBe(true);
    const copied = await secondResponse.json() as { canonical_yaml: string };
    expect(copied.canonical_yaml).toContain("kind: inline_sql");
    expect(copied.canonical_yaml).not.toContain("kind: saved_sql");
    const currentSavedSql = await request.get(
      getBackendUrl(`/spaces/${spaceId}/sql/${savedSql.id}`),
    );
    expect(await currentSavedSql.json()).toMatchObject({
      revision_id: latest.revision_id,
      sql: "SELECT CAST(99 AS BIGINT) AS pinned_value",
    });
  });

  test("Composition Studio responsive modes fit target viewports and support keyboard and touch", async ({ browser, context, request }) => {
    // The graph canvas and binding picker stay reachable on narrow touch
    // viewports without a second source-selection flow.
    test.setTimeout(120_000);
    const storageState = await context.storageState();
    const suffix = crypto.randomUUID().slice(0, 8);
    const compositionName = `Studio mobile ${suffix}`;
    const yaml = await completeCompositionGraphYaml(
      request,
      seed.spaceId,
      buildMobileStudioSeed(
        compositionName,
        seed.savedSqlId,
        seed.savedSqlRevisionId,
        seed.monthStart,
        seed.monthEnd,
      ),
    );
    const saved = await saveCompositionGraph(request, seed.spaceId, yaml);

    const mobileContext = await browser.newContext({
      storageState,
      viewport: { width: 390, height: 844 },
      hasTouch: true,
    });
    try {
      const page = await mobileContext.newPage();
      await page.goto(
        getFrontendUrl(
          `/spaces/${seed.spaceId}/compositions/${saved.composition_id}/${saved.revision_id}/edit`,
        ),
        { waitUntil: "domcontentloaded" },
      );
      const metricBlock = page.getByRole("button", {
        name: "metric, studio_total",
        exact: true,
      });
      await expect(metricBlock).toBeVisible({ timeout: 30_000 });
      await metricBlock.tap();
      await expect(page.getByRole("button", {
        name: "Change variable, studio_total",
        exact: true,
      })).toBeVisible();

      await page.getByRole("button", { name: "Add to design", exact: true })
        .first().click();
      const palette = page.getByRole("group", { name: "Add to design" });
      await palette.getByRole("button", { name: "Table", exact: true }).tap();
      const picker = page.getByRole("dialog", { name: "Table", exact: true });
      await expect(picker).toBeVisible();
      const viewportWidth = await page.evaluate(() => window.innerWidth);
      const pickerWidth = Number.parseFloat(
        await picker.evaluate((element) => getComputedStyle(element).width),
      );
      expect(pickerWidth).toBeLessThanOrEqual(viewportWidth - 32);
      const tabs = picker.getByRole("tablist", { name: "Variables" });
      for (const label of ["Variables", "Forms", "Saved SQL"]) {
        await expect(tabs.getByRole("tab", { name: label, exact: true }))
          .toBeVisible();
      }
      await page.keyboard.press("Escape");
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await expect(page.locator("#app")).not.toHaveAttribute("inert", "");
      await expect(
        page.getByRole("button", {
          name: "Add to design",
          exact: true,
        }).first(),
      ).toBeFocused();

      // 320px represents the effective content width at 200% browser zoom;
      // actual browser zoom and iPhone Safari remain manual device evidence.
      // Exercise every Studio arrangement at each responsive breakpoint.
      for (const width of [320, 375, 390, 760, 1280]) {
        await page.setViewportSize({ width, height: 844 });
        const modes = ["Design", "Variables"] as const;
        for (const mode of modes) {
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
          const pageWidth = await page.evaluate(() => ({
            client: document.documentElement.clientWidth,
            content: document.documentElement.scrollWidth,
          }));
          expect(pageWidth.content).toBeLessThanOrEqual(pageWidth.client);
          const modeOptions = page.getByRole("tablist", {
            name: "Studio mode",
          }).getByRole("tab");
          for (const modeOption of await modeOptions.all()) {
            if (!(await modeOption.isVisible())) continue;
            const height = await modeOption.evaluate((element) =>
              element.getBoundingClientRect().height
            );
            expect(height).toBeGreaterThanOrEqual(44);
          }
          await expect(page.getByRole("dialog")).toHaveCount(0);
          await expect(page.locator("#app")).not.toHaveAttribute("inert", "");
        }
      }
    } finally {
      await mobileContext.close();
    }
  });
});
