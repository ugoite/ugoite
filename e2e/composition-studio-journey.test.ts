import {
  type APIRequestContext,
  type Browser,
  type BrowserContext,
  expect,
  type Page,
  type Request,
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
import { parse as parseYaml } from "yaml";

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
  await expect(dialog).toHaveCount(0, { timeout: 15_000 });
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
  await page.getByLabel("Variable name", { exact: true }).fill(variableId);
  await page.getByLabel("Variable name", { exact: true }).press("Enter");
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
  await page.getByRole("group", { name: "Add to design" })
    .getByLabel("Input", { exact: true }).selectOption({ label: variableId });
}

async function addStudioText(page: Page, variableId: string): Promise<void> {
  await page.getByRole("tablist", { name: "Studio mode" })
    .getByRole("tab", { name: "Design", exact: true }).click();
  await page.getByRole("group", { name: "Add to design" })
    .getByRole("button", { name: "Text", exact: true }).click();
  const textBlock = page.getByRole("button", {
    name: "Edit text",
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
      label_source: "variable",
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

    const currentSourceResponse = await request.get(
      getBackendUrl(`/spaces/${seed.spaceId}/sql/${seed.savedSqlId}`),
    );
    expect(currentSourceResponse.ok()).toBe(true);
    const currentSource = await currentSourceResponse.json() as {
      name: string;
      kind: string;
      sql: string;
      revision_id: string;
    };
    expect(currentSource.revision_id).toBe(seed.savedSqlRevisionId);
    const revisedSourceSql = currentSource.sql.replace(
      " AS total,",
      " * 2 AS total,",
    );
    expect(revisedSourceSql).not.toBe(currentSource.sql);
    const sourceUpdate = await request.put(
      getBackendUrl(`/spaces/${seed.spaceId}/sql/${seed.savedSqlId}`),
      {
        data: {
          name: currentSource.name,
          kind: currentSource.kind,
          sql: revisedSourceSql,
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
          parent_revision_id: currentSource.revision_id,
        },
      },
    );
    expect(sourceUpdate.ok(), await sourceUpdate.text()).toBe(true);
    const updatedSource = await sourceUpdate.json() as {
      revision_id: string;
    };
    expect(updatedSource.revision_id).not.toBe(seed.savedSqlRevisionId);

    const latestSourceResponse = await request.get(
      getBackendUrl(`/spaces/${seed.spaceId}/sql/${seed.savedSqlId}`),
    );
    expect(latestSourceResponse.ok()).toBe(true);
    expect(await latestSourceResponse.json()).toMatchObject({
      revision_id: updatedSource.revision_id,
      sql: revisedSourceSql,
    });
    const latestSourceResultResponse = await request.post(
      getBackendUrl(`/spaces/${seed.spaceId}/sql/query`),
      {
        data: {
          sql: revisedSourceSql,
          parameters: {
            month_start: seed.monthStart,
            month_end: seed.monthEnd,
          },
          parameter_types: { month_start: "date", month_end: "date" },
          saved_sql: {
            id: seed.savedSqlId,
            revision_id: updatedSource.revision_id,
          },
          limit: 100,
        },
      },
    );
    expect(latestSourceResultResponse.ok(), await latestSourceResultResponse.text())
      .toBe(true);
    const latestSourceResult = await latestSourceResultResponse.json() as {
      columns: string[];
      rows: Array<Record<string, unknown>>;
    };
    expect(latestSourceResult.columns).toEqual(["total", "quantity"]);
    expect(latestSourceResult.rows).toEqual([{ total: 40, quantity: 3 }]);

    const stillPinnedResponse = await request.get(
      getBackendUrl(
        `/spaces/${seed.spaceId}/compositions/${compositionId}/history/${firstRevisionId}`,
      ),
    );
    expect(stillPinnedResponse.ok()).toBe(true);
    const stillPinned = await stillPinnedResponse.json() as StudioRawRevision;
    expect(String(stillPinned.fields["spec"])).toContain(
      `revision_id: ${seed.savedSqlRevisionId}`,
    );
    expect(String(stillPinned.fields["spec"])).not.toContain(
      `revision_id: ${updatedSource.revision_id}`,
    );

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
        page.getByRole("group", { name: "Add to design", exact: true })
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
      await expect(page.getByPlaceholder("Composition name")).toBeVisible();
      await expect(page.getByRole("complementary", { name: "Inspector" }))
        .toHaveCount(0);
      for (const name of ["Table", "Metric", "Input", "Text"]) {
        await expect(
          page.getByRole("group", { name: "Add to design" }).getByRole(
            "button",
            { name, exact: true },
          ),
        ).toBeVisible();
      }
      await page.setViewportSize({ width: 1280, height: 900 });
      await page.screenshot({
        path: test.info().outputPath("new-composition-1280.png"),
        fullPage: true,
      });
      await page.setViewportSize({ width: 390, height: 844 });
      await page.screenshot({
        path: test.info().outputPath("new-composition-390.png"),
        fullPage: true,
      });
      await page.setViewportSize({ width: 1280, height: 900 });
      await page.getByLabel("Name", { exact: true }).fill(
        `${seed.toolName} Form bindings`,
      );
      const palette = page.getByRole("group", { name: "Add to design" });
      await palette.getByRole("button", { name: "Table", exact: true }).click();
      const tablePicker = page.getByRole("dialog", {
        name: "Table",
        exact: true,
      });
      await page.screenshot({
        path: test.info().outputPath("new-table-picker.png"),
        fullPage: true,
      });
      await expect(tablePicker.getByRole("status")).toContainText(
        "No compatible variables",
      );
      await tablePicker.getByRole("tab", { name: "Forms", exact: true })
        .click();
      await tablePicker.getByRole("button", {
        name: seed.formName,
        exact: true,
      }).click();
      await expect(tablePicker).toHaveCount(0);
      await expect(page.getByRole("button", { name: /Change variable/ }))
        .toHaveCount(0);
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
      await expect(
        page.getByRole("region", {
          name: `${formSlug}_sum_amount`,
          exact: true,
        }),
      )
        .toContainText("50");
      await page.getByRole("button", {
        name: `table, ${formSlug}_rows`,
        exact: true,
      }).click();
      const moveLater = page.getByRole("button", {
        name: "Move later",
        exact: true,
      });
      await moveLater.focus();
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
        const tableRegion = page.getByRole("region", {
          name: `${formSlug}_rows results`,
        });
        const dimensions = await tableRegion.evaluate((element) => ({
          width: element.clientWidth,
          content: element.scrollWidth,
          whitespace: getComputedStyle(element.querySelector("th")!).whiteSpace,
        }));
        expect(dimensions.content).toBeGreaterThan(dimensions.width);
        expect(dimensions.whitespace).toBe("nowrap");
      }
      await page.screenshot({
        path: test.info().outputPath("composition-live-design-390.png"),
        fullPage: true,
      });
      await test.info().attach("Composition live Design at 390px", {
        path: test.info().outputPath("composition-live-design-390.png"),
        contentType: "image/png",
      });
      await page.getByRole("button", { name: "Save", exact: true }).click();
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${seed.spaceId}/compositions/[^/]+/[^/]+$`),
        { timeout: 30_000 },
      );
      await expect(
        page.getByRole("table", { name: `${formSlug}_rows results` }),
      ).toContainText(seed.purposes[0], { timeout: 30_000 });
      await expect(
        page.getByRole("region", {
          name: `${formSlug}_sum_amount`,
          exact: true,
        }),
      )
        .toContainText("50");
      const saved = revisionFromUrl(page.url());
      const runtimeBlocks = page.locator(
        ".compositionGraphRender .compositionGraphCanvasRow .compositionGraphValue",
      );
      await expect(runtimeBlocks.first()).toContainText("50");
      await expect(runtimeBlocks.nth(1).getByRole("table")).toContainText(
        seed.purposes[0],
      );
      const response = await request.get(
        getBackendUrl(
          `/spaces/${seed.spaceId}/compositions/${saved.compositionId}/history/${saved.revisionId}`,
        ),
      );
      expect(response.ok()).toBe(true);
      const revision = await response.json() as StudioRawRevision;
      const savedYaml = String(revision.fields["spec"]);
      expect(savedYaml).toContain(String(form?.id));
      const savedDocument = parseYaml(savedYaml) as {
        spec?: {
          components?: Array<{
            id?: string;
            kind?: string;
            variable?: string;
          }>;
          layout?: {
            rows?: Array<{
              items?: Array<{ component?: string }>;
            }>;
          };
        };
      };
      const componentById = new Map(
        (savedDocument.spec?.components ?? []).map((component) => [
          component.id,
          component,
        ]),
      );
      const savedOrder = (savedDocument.spec?.layout?.rows ?? [])
        .flatMap((row) => row.items ?? [])
        .map((item) => componentById.get(item.component));
      expect(savedOrder.map((component) => [
        component?.kind,
        component?.variable,
      ])).toEqual([
        ["metric", `${formSlug}_sum_amount`],
        ["table", `${formSlug}_rows`],
      ]);
      const components =
        savedYaml.split("components:")[1]?.split("layout:")[0] ??
          "";

      expect(savedYaml).toContain(
        `SELECT * FROM "${seed.formName}"`,
      );
      expect(savedYaml).toContain(
        `SUM("${amountColumn}") AS value FROM "${seed.formName}"`,
      );
      expect(components).toContain(`variable: ${formSlug}_rows`);
      expect(components).toContain(
        `variable: ${formSlug}_sum_amount`,
      );
      expect(components).not.toMatch(/\b(?:form_id|source_id|source):/);

      // Reopen the exact revision in the editor, verify the saved display
      // order, explicitly run the Form-backed query, then return to its saved
      // runtime view and confirm the same order and rows.
      const editUrl =
        `/spaces/${seed.spaceId}/compositions/${saved.compositionId}/${saved.revisionId}/edit`;
      await page.goto(getFrontendUrl(editUrl), {
        waitUntil: "domcontentloaded",
      });
      await expect(page).toHaveURL(new RegExp(
        `/spaces/${seed.spaceId}/compositions/${saved.compositionId}/${saved.revisionId}/edit$`,
      ));
      const editedBlocks = page.locator(".compositionGraphDesignValue");
      await expect(editedBlocks.first()).toContainText("50", {
        timeout: 30_000,
      });
      await expect(editedBlocks.nth(1).getByRole("table")).toContainText(
        seed.purposes[0],
        { timeout: 30_000 },
      );
      await page.getByRole("tablist", { name: "Studio mode" })
        .getByRole("tab", { name: "Variables", exact: true }).click();
      await page.getByRole("button", { name: "Show variables", exact: true })
        .click();
      await page.getByRole("button", {
        name: `${formSlug}_rows`,
        exact: true,
      }).click();
      await page.getByRole("button", { name: "Run Query", exact: true }).click();
      const explicitQueryResult = page.getByRole("table");
      await expect(explicitQueryResult).toContainText(seed.purposes[0], {
        timeout: 30_000,
      });
      await expect(explicitQueryResult).toContainText(seed.purposes[1]);

      await page.goto(getFrontendUrl(
        `/spaces/${seed.spaceId}/compositions/${saved.compositionId}/${saved.revisionId}`,
      ), { waitUntil: "domcontentloaded" });
      await expect(page).toHaveURL(new RegExp(
        `/spaces/${seed.spaceId}/compositions/${saved.compositionId}/${saved.revisionId}$`,
      ));
      const reopenedRuntimeBlocks = page.locator(
        ".compositionGraphRender .compositionGraphCanvasRow .compositionGraphValue",
      );
      await expect(reopenedRuntimeBlocks.first()).toContainText("50", {
        timeout: 30_000,
      });
      await expect(reopenedRuntimeBlocks.nth(1).getByRole("table"))
        .toContainText(seed.purposes[0], { timeout: 30_000 });
      await expect(reopenedRuntimeBlocks.nth(1).getByRole("table"))
        .toContainText(seed.purposes[1]);

    } finally {
      await ownerContext.close();
    }
  });

  test("Changing a Form-backed Table source preserves the Metric and query through save and reopen", async ({ browser, context, request }) => {
    const storageState = await context.storageState();
    const ownerContext = await openOwnerContext(browser, storageState);
    try {
      const formsResponse = await request.get(
        getBackendUrl(`/spaces/${seed.spaceId}/forms`),
      );
      expect(formsResponse.ok()).toBe(true);
      const forms = await formsResponse.json() as Array<{
        id?: string;
        name?: string;
      }>;
      const form = forms.find((candidate) => candidate.name === seed.formName);
      expect(form?.id).toBeTruthy();
      const formSlug = seed.formName.normalize("NFKD").toLowerCase()
        .replace(/\p{M}/gu, "")
        .replace(/[^a-z0-9]+/g, "_")
        .replace(/^_+|_+$/g, "");
      const replacementFormName = `StudioReplacement${crypto.randomUUID().slice(0, 8)}`;
      const replacementFormResponse = await request.post(
        getBackendUrl(`/spaces/${seed.spaceId}/forms`),
        {
          data: {
            name: replacementFormName,
            version: 1,
            template: `# ${replacementFormName}\n\n## Label\n`,
            fields: { Label: { type: "string", required: true } },
          },
        },
      );
      expect([200, 201]).toContain(replacementFormResponse.status());
      const replacementEntryResponse = await request.post(
        getBackendUrl(`/spaces/${seed.spaceId}/entries`),
        {
          data: {
            form: replacementFormName,
            fields: { Label: "Replacement Form row" },
          },
        },
      );
      expect(replacementEntryResponse.status()).toBe(201);
      const replacementFormsResponse = await request.get(
        getBackendUrl(`/spaces/${seed.spaceId}/forms`),
      );
      expect(replacementFormsResponse.ok()).toBe(true);
      const replacementForms = await replacementFormsResponse.json() as Array<{
        id?: string;
        name?: string;
      }>;
      const replacementForm = replacementForms.find((candidate) =>
        candidate.name === replacementFormName
      );
      expect(replacementForm?.id).toBeTruthy();
      const replacementSlug = replacementFormName.normalize("NFKD")
        .toLowerCase()
        .replace(/\p{M}/gu, "")
        .replace(/[^a-z0-9]+/g, "_")
        .replace(/^_+|_+$/g, "");

      const page = await ownerContext.newPage();
      await page.goto(
        getFrontendUrl(`/spaces/${seed.spaceId}/compositions/new`),
        { waitUntil: "domcontentloaded" },
      );
      await expect(page.getByPlaceholder("Composition name")).toBeVisible();
      await expect(page.getByRole("complementary", { name: "Inspector" }))
        .toHaveCount(0);
      await page.getByLabel("Name", { exact: true }).fill(
        `${seed.toolName} Form source change`,
      );
      const palette = page.getByRole("group", { name: "Add to design" });
      for (const name of ["Table", "Metric"]) {
        await expect(palette.getByRole("button", { name, exact: true }))
          .toBeVisible();
      }
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
      await palette.getByRole("button", { name: "Metric", exact: true }).click();
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
      await metricPicker.getByRole("combobox", {
        name: "Aggregation",
        exact: true,
      }).selectOption("sum");
      await metricPicker.getByRole("combobox", {
        name: "Column",
        exact: true,
      }).selectOption({ label: "Amount" });
      await metricPicker.getByRole("button", { name: "Add", exact: true })
        .click();
      await expect(metricPicker).toHaveCount(0);
      await page.getByRole("button", { name: "Save", exact: true }).click();
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${seed.spaceId}/compositions/[^/]+/[^/]+$`),
        { timeout: 30_000 },
      );
      const saved = revisionFromUrl(page.url());
      const savedDocumentResponse = await request.get(
        getBackendUrl(
          `/spaces/${seed.spaceId}/compositions/${saved.compositionId}/history/${saved.revisionId}`,
        ),
      );
      expect(savedDocumentResponse.ok()).toBe(true);
      const savedRevision = await savedDocumentResponse.json() as StudioRawRevision;
      const savedDocument = parseYaml(
        String(savedRevision.fields["spec"]),
      ) as {
        spec?: {
          variables?: Array<{
            id?: string;
            query?: {
              kind?: string;
              sql?: string;
              form_bindings?: Array<{ relation: string; form_id: string }>;
            };
          }>;
          components?: Array<{
            id?: string;
            kind?: string;
            variable?: string;
          }>;
        };
      };
      const compositionId = saved.compositionId;
      const initialRevisionId = saved.revisionId;
      const savedEditUrl =
        `/spaces/${seed.spaceId}/compositions/${compositionId}/${initialRevisionId}/edit`;
      await page.goto(getFrontendUrl(savedEditUrl), {
        waitUntil: "domcontentloaded",
      });
      const sourceTableButton = page.getByRole("button", {
        name: `table, ${formSlug}_rows`,
        exact: true,
      });
      await expect(sourceTableButton).toBeVisible({ timeout: 30_000 });
      await expect(page.getByRole("button", {
        name: `metric, ${formSlug}_sum_amount`,
        exact: true,
      })).toBeVisible();
      await sourceTableButton.click();
      const inspector = page.getByRole("complementary", {
        name: "Selected component",
        exact: true,
      });
      await inspector.getByRole("button", {
        name: `Change variable, ${formSlug}_rows`,
        exact: true,
      }).click();
      const rebindPicker = page.getByRole("dialog", {
        name: "Table",
        exact: true,
      });
      await rebindPicker.getByRole("tab", {
        name: "Forms",
        exact: true,
      }).click();
      await rebindPicker.getByRole("button", {
        name: replacementFormName,
        exact: true,
      }).click();
      await expect(rebindPicker).toHaveCount(0, { timeout: 30_000 });
      await expect(page.getByRole("button", {
        name: `table, ${replacementSlug}_rows`,
        exact: true,
      })).toBeVisible();
      await expect(page.getByRole("button", {
        name: `metric, ${formSlug}_sum_amount`,
        exact: true,
      })).toBeVisible();
      await page.getByRole("button", { name: "Save", exact: true }).click();
      await expect(page).toHaveURL(
        new RegExp(
          `/spaces/${seed.spaceId}/compositions/${compositionId}/[^/]+$`,
        ),
        { timeout: 30_000 },
      );
      const rebound = revisionFromUrl(page.url());
      expect(rebound.revisionId).not.toBe(initialRevisionId);

      const reboundRevisionResponse = await request.get(
        getBackendUrl(
          `/spaces/${seed.spaceId}/compositions/${rebound.compositionId}/history/${rebound.revisionId}`,
        ),
      );
      expect(reboundRevisionResponse.ok()).toBe(true);
      const reboundRevision = await reboundRevisionResponse.json() as StudioRawRevision;
      const reboundSpec = parseYaml(
        String(reboundRevision.fields["spec"]),
      ) as {
        spec?: {
          variables?: Array<{
            id?: string;
            query?: {
              kind?: string;
              sql?: string;
              form_bindings?: Array<{ relation: string; form_id: string }>;
            };
          }>;
          components?: Array<{
            id?: string;
            kind?: string;
            variable?: string;
          }>;
        };
      };
      const reboundVariables = reboundSpec.spec?.variables ?? [];
      const originalFormQuery = reboundVariables.find((variable) =>
        variable.id === `${formSlug}_rows`
      );
      const originalSavedFormQuery = savedDocument.spec?.variables?.find(
        (variable) => variable.id === `${formSlug}_rows`,
      );
      expect(originalFormQuery?.query).toMatchObject({
        kind: "inline_sql",
        sql: `SELECT * FROM ${sqlIdentifier(seed.formName)}`,
        form_bindings: [{ relation: seed.formName, form_id: form!.id! }],
      });
      expect(originalFormQuery?.query).toEqual(originalSavedFormQuery?.query);
      expect(reboundVariables.some((variable) =>
        variable.id === `${replacementSlug}_rows` &&
        variable.query?.form_bindings?.some((binding) =>
          binding.relation === replacementFormName &&
          binding.form_id === replacementForm!.id
        )
      )).toBe(true);
      const reboundComponents = reboundSpec.spec?.components ?? [];
      expect(reboundComponents.map(({ id, kind }) => `${kind}:${id}`).sort())
        .toEqual((savedDocument.spec?.components ?? [])
          .map(({ id, kind }) => `${kind}:${id}`)
          .sort());
      expect(reboundComponents.find((component) =>
        component.kind === "metric"
      )?.variable).toBe(`${formSlug}_sum_amount`);
      expect(reboundComponents.find((component) =>
        component.kind === "table"
      )?.variable).toBe(`${replacementSlug}_rows`);
      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(page.getByRole("region", {
        name: `${formSlug}_sum_amount`,
        exact: true,
      })).toContainText("50", { timeout: 30_000 });
      await expect(page.getByRole("table", {
        name: `${replacementSlug}_rows results`,
      })).toContainText("Replacement Form row", { timeout: 30_000 });
    } finally {
      await ownerContext.close();
    }
  });

  test("Composition Studio creates a populated Form table from a fresh unnamed Composition and reopens it", async ({ browser, context, request }) => {
    test.setTimeout(120_000);
    const spaceId = await createSpace(request);
    const suffix = crypto.randomUUID().slice(0, 8);
    const formName = `TableRecords${suffix}`;
    const compositionName = `Form table ${suffix}`;
    const formResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/forms`),
      {
        data: {
          name: formName,
          version: 1,
          template: `# ${formName}\n\n## Name\n\n## Quantity\n\n## Date\n`,
          fields: {
            Name: { type: "string", required: true },
            Quantity: { type: "integer", required: true },
            Date: { type: "date", required: true },
          },
        },
      },
    );
    expect([200, 201]).toContain(formResponse.status());

    const entryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      {
        data: {
          form: formName,
          fields: {
            Name: "First populated row",
            Quantity: 17,
            Date: "2026-10-27",
          },
        },
      },
    );
    expect(entryResponse.status()).toBe(201);

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
    for (const field of ["Name", "Quantity", "Date"]) {
      expect(form?.fields?.[field]?.sql_column).toBeTruthy();
    }
    const formSlug = formName.normalize("NFKD").toLowerCase()
      .replace(/\p{M}/gu, "").replace(/[^a-z0-9]+/g, "_")
      .replace(/^_+|_+$/g, "");

    const ownerContext = await openOwnerContext(
      browser,
      await context.storageState(),
    );
    try {
      const page = await ownerContext.newPage();
      const apiSpacePrefix = new URL(
        getBackendUrl(`/spaces/${spaceId}/`),
      ).pathname;
      const failingApiResponses: Array<{ path: string; status: number }> = [];
      const successfulApiPaths: string[] = [];
      page.on("response", (response) => {
        const path = new URL(response.url()).pathname;
        if (
          path.startsWith(apiSpacePrefix) &&
          /\/(?:compositions|forms|sql|entries)(?:\/|$)/.test(path)
        ) {
          if (!response.ok()) {
            failingApiResponses.push({ path, status: response.status() });
          } else {
            successfulApiPaths.push(path);
          }
        }
      });
      await page.goto(
        getFrontendUrl(`/spaces/${spaceId}/compositions/new`),
        { waitUntil: "domcontentloaded" },
      );
      await expect(page.getByPlaceholder("Composition name")).toBeVisible();
      await expect(page.getByPlaceholder("Composition name")).toHaveValue("");
      const palette = page.getByRole("group", { name: "Add to design" });
      await palette.getByRole("button", { name: "Table", exact: true })
        .click();
      const picker = page.getByRole("dialog", { name: "Table", exact: true });
      await picker.getByRole("tab", { name: "Forms", exact: true }).click();
      await picker.getByRole("button", { name: formName, exact: true }).click();
      await expect(picker).toHaveCount(0, { timeout: 30_000 });
      const designTable = page.getByRole("table", {
        name: `${formSlug}_rows results`,
      });
      await expect(designTable).toBeVisible({ timeout: 30_000 });
      expect(failingApiResponses).toEqual([]);
      expect(successfulApiPaths).toContain(
        `${apiSpacePrefix}compositions/preview`,
      );
      expect(successfulApiPaths).toContain(
        `${apiSpacePrefix}compositions/evaluate`,
      );
      await expect(designTable.locator("thead")).toContainText("Name");
      await expect(designTable.locator("thead")).toContainText("Quantity");
      await expect(designTable.locator("thead")).toContainText("Date");
      await expect(designTable.locator("thead")).not.toContainText("_ugoite_");
      await expect(designTable.locator("thead")).not.toContainText("field_");
      const populatedRow = designTable.locator("tbody tr");
      await expect(populatedRow).toHaveCount(1, { timeout: 30_000 });
      await expect(populatedRow).toContainText("First populated row");
      await expect(populatedRow).toContainText("17");
      await expect(populatedRow).toContainText("2026-10-27");

      await page.getByLabel("Name", { exact: true }).fill(compositionName);
      await page.getByRole("button", { name: "Save", exact: true }).click();
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${spaceId}/compositions/[^/]+/[^/]+$`),
        { timeout: 30_000 },
      );
      const saved = revisionFromUrl(page.url());
      const revisionResponse = await request.get(
        getBackendUrl(
          `/spaces/${spaceId}/compositions/${saved.compositionId}/history/${saved.revisionId}`,
        ),
      );
      expect(revisionResponse.ok()).toBe(true);
      const revision = await revisionResponse.json() as StudioRawRevision;
      const savedSpec = parseYaml(String(revision.fields["spec"])) as {
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
      const savedFormQuery = savedSpec.spec?.variables?.find((variable) =>
        variable.id === `${formSlug}_rows` &&
        variable.query?.kind === "inline_sql"
      )?.query;
      expect(savedFormQuery).toBeDefined();
      expect(savedFormQuery?.sql).toBe(
        `SELECT * FROM ${sqlIdentifier(formName)}`,
      );
      expect(savedFormQuery?.form_bindings).toEqual([{
        relation: formName,
        form_id: form!.id!,
      }]);
      expect(successfulApiPaths).toContain(
        `${apiSpacePrefix}compositions`,
      );
      await page.reload({ waitUntil: "domcontentloaded" });
      const reopenedTable = page.getByRole("table", {
        name: `${formSlug}_rows results`,
      });
      await expect(reopenedTable).toBeVisible({ timeout: 30_000 });
      const reopenedRow = reopenedTable.locator("tbody tr");
      await expect(reopenedRow).toHaveCount(1, { timeout: 30_000 });
      await expect(reopenedRow).toContainText("First populated row");
      await expect(reopenedRow).toContainText("17");
      await expect(reopenedRow).toContainText("2026-10-27");
      expect(failingApiResponses).toEqual([]);
    } finally {
      await ownerContext.close();
    }
  });

  test("Composition Studio adds a no-entry Form table beside an existing query and reopens it", async ({ browser, context, request }) => {
    test.setTimeout(120_000);
    const spaceId = await createSpace(request);
    const suffix = crypto.randomUUID().slice(0, 8);
    const formName = `EmptyRecords${suffix}`;
    const sqlName = `Existing composition query ${suffix}`;
    const compositionName = `Form table composition ${suffix}`;
    const formResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/forms`),
      {
        data: {
          name: formName,
          version: 1,
          template: `# ${formName}\n\n## Name\n\n## Quantity\n`,
          fields: {
            Name: { type: "string", required: true },
            Quantity: { type: "integer", required: true },
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
    expect(form?.fields?.Name?.sql_column).toBeTruthy();
    expect(form?.fields?.Quantity?.sql_column).toBeTruthy();
    const formSlug = formName.normalize("NFKD").toLowerCase()
      .replace(/\p{M}/gu, "").replace(/[^a-z0-9]+/g, "_")
      .replace(/^_+|_+$/g, "");
    const queryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: {
          name: sqlName,
          kind: "user-query",
          sql: `SELECT * FROM ${sqlIdentifier(form!.sql_relation!)}`,
        },
      },
    );
    expect([200, 201]).toContain(queryResponse.status());

    const ownerContext = await openOwnerContext(
      browser,
      await context.storageState(),
    );
    try {
      const page = await ownerContext.newPage();
      await page.goto(
        getFrontendUrl(`/spaces/${spaceId}/compositions/new`),
        { waitUntil: "domcontentloaded" },
      );
      const palette = page.getByRole("group", { name: "Add to design" });

      await palette.getByRole("button", { name: "Table", exact: true })
        .click();
      const tablePicker = page.getByRole("dialog", {
        name: "Table",
        exact: true,
      });
      await tablePicker.getByRole("tab", { name: "Saved SQL", exact: true })
        .click();
      await expect(tablePicker.getByRole("button", {
        name: sqlName,
        exact: true,
      })).toBeVisible();
      await tablePicker.getByRole("tab", { name: "Forms", exact: true })
        .click();
      await tablePicker.getByRole("button", { name: formName, exact: true })
        .click();
      await expect(tablePicker).toHaveCount(0);
      await expect(page.getByRole("button", { name: "Save", exact: true }))
        .toBeDisabled();
      const designTable = page.getByRole("table", {
        name: `${formSlug}_rows results`,
      });
      await expect(designTable).toBeVisible({ timeout: 30_000 });
      await expect(designTable.locator("tbody tr")).toHaveCount(0);

      await page.getByRole("tablist", { name: "Studio mode" })
        .getByRole("tab", { name: "Variables", exact: true }).click();
      await page.getByRole("button", { name: `${formSlug}_rows`, exact: true })
        .click();
      await page.getByRole("button", { name: "Run Query", exact: true })
        .click();
      await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(0, {
        timeout: 30_000,
      });
      await page.getByRole("tablist", { name: "Studio mode" })
        .getByRole("tab", { name: "Design", exact: true }).click();
      await page.getByLabel("Name", { exact: true }).fill(compositionName);

      await page.getByRole("button", { name: "Save", exact: true }).click();
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${spaceId}/compositions/[^/]+/[^/]+$`),
        { timeout: 30_000 },
      );
      const saved = revisionFromUrl(page.url());
      const revisionResponse = await request.get(
        getBackendUrl(
          `/spaces/${spaceId}/compositions/${saved.compositionId}/history/${saved.revisionId}`,
        ),
      );
      expect(revisionResponse.ok()).toBe(true);
      const revision = await revisionResponse.json() as StudioRawRevision;
      const savedSpec = parseYaml(String(revision.fields["spec"])) as {
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
      const savedFormQuery = savedSpec.spec?.variables?.find((variable) =>
        variable.id === `${formSlug}_rows` &&
        variable.query?.kind === "inline_sql"
      )?.query;
      expect(savedFormQuery).toBeDefined();
      expect(savedFormQuery?.sql).toBe(
        `SELECT * FROM ${sqlIdentifier(formName)}`,
      );
      expect(savedFormQuery?.form_bindings).toEqual([{
        relation: formName,
        form_id: form!.id!,
      }]);
      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(page.getByRole("table", {
        name: `${formSlug}_rows results`,
      })).toBeVisible({ timeout: 30_000 });
      await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(0);
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
      await page.getByRole("tab", { name: "Variables", exact: true }).click();
      await page.getByRole("button", { name: "Add variable", exact: true })
        .click();
      await page.getByRole("group", { name: "Add variable" })
        .getByRole("button", { name: "Value", exact: true }).click();
      const pendingValueName = page.getByLabel("Variable name", { exact: true });
      await expect(pendingValueName).toBeFocused();
      await page.screenshot({
        path: test.info().outputPath("variable-name-value-desktop.png"),
        fullPage: true,
      });
      await pendingValueName.fill("floor_limit");
      await pendingValueName.press("Enter");
      await expect(page.getByRole("button", { name: "floor_limit", exact: true }))
        .toBeVisible();
      await expect(page.getByLabel("Value", { exact: true })).toBeVisible();
      await page.getByRole("button", { name: "Add variable", exact: true })
        .click();
      await page.getByRole("group", { name: "Add variable" })
        .getByRole("button", { name: "Value", exact: true }).click();
      const duplicateValueName = page.getByLabel("Variable name", { exact: true });
      await duplicateValueName.fill("floor_limit");
      await expect(page.getByText("A variable with this name already exists."))
        .toBeVisible();
      await expect(page.getByRole("button", { name: "Add", exact: true }))
        .toBeDisabled();
      await page.screenshot({
        path: test.info().outputPath("variable-name-duplicate-desktop.png"),
        fullPage: true,
      });
      await page.getByRole("button", { name: "Cancel", exact: true }).click();
      await page.getByRole("button", { name: "Remove floor_limit" }).click();
      await page.getByRole("tablist", { name: "Studio mode" })
        .getByRole("tab", { name: "Design", exact: true }).click();
      await page.getByRole("button", { name: "Input", exact: true }).click();
      const designPendingName = page.getByLabel("Variable name", { exact: true });
      await expect(designPendingName).toBeFocused();
      await designPendingName.fill("draft_label");
      await expect(page.getByRole("tab", { name: "Variables", exact: true }))
        .toBeDisabled();
      await expect(designPendingName).toHaveValue("draft_label");
      await expect(page.getByRole("button", { name: "Save", exact: true }))
        .toHaveAttribute("title", "Add or cancel the variable before saving.");
      await expect(page.getByText(
        "Finish or cancel adding this variable before switching modes.",
        { exact: true },
      )).toBeVisible();
      await page.screenshot({
        path: test.info().outputPath("variable-name-design-desktop.png"),
        fullPage: true,
      });
      await page.getByRole("button", { name: "Cancel", exact: true }).click();
      for (const width of [320, 375, 390]) {
        await page.setViewportSize({ width, height: 844 });
        await page.getByRole("tablist", { name: "Studio mode" })
          .getByRole("tab", { name: "Variables", exact: true }).click();
        await page.getByRole("button", { name: "Show variables", exact: true })
          .click();
        await page.getByRole("button", { name: "Add variable", exact: true })
          .click();
        const mobileValueOption = page.getByRole("group", {
          name: "Add variable",
        }).getByRole("button", { name: "Value", exact: true });
        await expect(mobileValueOption).toBeVisible();
        const mobileValueBox = await mobileValueOption.boundingBox();
        expect(mobileValueBox).not.toBeNull();
        expect(mobileValueBox!.x).toBeGreaterThanOrEqual(0);
        expect(mobileValueBox!.y).toBeGreaterThanOrEqual(0);
        expect(mobileValueBox!.x + mobileValueBox!.width)
          .toBeLessThanOrEqual(width);
        expect(mobileValueBox!.y + mobileValueBox!.height)
          .toBeLessThanOrEqual(844);
        expect(
          await page.evaluate(() =>
            document.documentElement.scrollWidth <=
              document.documentElement.clientWidth
          ),
        ).toBe(true);
        await mobileValueOption.click();
        await expect(page.getByLabel("Variable name", { exact: true }))
          .toBeFocused();
        if (width === 390) {
          await page.screenshot({
            path: test.info().outputPath("variable-name-value-390px.png"),
            fullPage: true,
          });
        }
        await page.getByRole("button", { name: "Cancel", exact: true }).click();
        await page.getByRole("tablist", { name: "Studio mode" })
          .getByRole("tab", { name: "Design", exact: true }).click();
        await page.getByRole("button", { name: "Input", exact: true }).click();
        await expect(page.getByLabel("Variable name", { exact: true }))
          .toBeFocused();
        if (width === 390) {
          await page.screenshot({
            path: test.info().outputPath("variable-name-design-390px.png"),
            fullPage: true,
          });
        }
        await page.getByRole("button", { name: "Cancel", exact: true }).click();
      }
      await page.setViewportSize({ width: 1280, height: 900 });
      await page.getByRole("tablist", { name: "Studio mode" })
        .getByRole("tab", { name: "Variables", exact: true }).click();
      await page.getByRole("button", { name: "Add variable", exact: true })
        .click();
      await page.getByRole("group", { name: "Add variable" })
        .getByRole("button", { name: "Input", exact: true }).click();
      const pendingName = page.getByLabel("Variable name", { exact: true });
      await expect(pendingName).toBeFocused();
      await expect(page.getByRole("button", { name: "Save", exact: true }))
        .toBeDisabled();
      await expect(page.getByRole("button", { name: "input_1", exact: true }))
        .toHaveCount(0);
      await pendingName.fill("discarded");
      await page.getByRole("button", { name: "Cancel", exact: true }).click();
      await expect(page.getByRole("button", { name: "Save", exact: true }))
        .toBeDisabled();
      await expect(page.getByRole("button", { name: "input_1", exact: true }))
        .toHaveCount(0);
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
      const savedYaml = String(exactRevision.fields["spec"]);
      expect(savedYaml).toContain(
        `revision_id: ${seed.savedSqlRevisionId}`,
      );
      expect(savedYaml).not.toMatch(
        /\bsource_id\b|\bsource:/,
      );
      const savedDocument = parseYaml(savedYaml) as {
        spec?: {
          variables?: Array<{
            id?: string;
            kind?: string;
            default?: { kind?: string; value?: unknown };
          }>;
          components?: Array<Record<string, unknown> & {
            id?: string;
            kind?: string;
            variable?: string;
          }>;
        };
      };
      const savedVariables = savedDocument.spec?.variables ?? [];
      const savedComponents = savedDocument.spec?.components ?? [];
      const variableIds = new Set(savedVariables.map(({ id }) => id));
      expect(savedVariables.find(({ id }) => id === "minimum")).toMatchObject({
        kind: "input",
        default: { kind: "integer", value: 0 },
      });
      expect(savedComponents).toHaveLength(3);
      for (const component of savedComponents) {
        expect(Object.keys(component).sort()).toEqual(
          ["id", "kind", "variable"],
        );
        expect(component.variable).toBeTruthy();
        expect(variableIds.has(component.variable)).toBe(true);
      }
      expect(savedDocument.spec).not.toHaveProperty("input_values");
      expect(savedDocument.spec).not.toHaveProperty("results");
      expect(savedDocument.spec).not.toHaveProperty("preview");
    } finally {
      await buildContext.close();
    }

    const journeyContext = await openOwnerContext(browser, storageState);
    try {
      const page = await journeyContext.newPage();
      const entryIdsBeforeEvaluation = await snapshotStudioEntryIds(
        request,
        seed.spaceId,
      );
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
      expect(await snapshotStudioEntryIds(request, seed.spaceId)).toEqual(
        entryIdsBeforeEvaluation,
      );
    } finally {
      await journeyContext.close();
    }
  });

  test("Design preview recomputes only a dependent query and keeps unrelated results", async ({ page, request }) => {
    test.setTimeout(60_000);
    const suffix = crypto.randomUUID().slice(0, 8);
    const spaceId = seed.spaceId;
    const formName = `DesignDependency${suffix}`;
    const dependentSqlName = `Design dependent count ${suffix}`;
    const independentSqlName = `Design independent rows ${suffix}`;
    const formResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/forms`),
      {
        data: {
          name: formName,
          version: 1,
          template: `# ${formName}\n\n## Merchant\n\n## Amount\n`,
          fields: {
            Merchant: { type: "string", required: true },
            Amount: { type: "double", required: true },
          },
        },
      },
    );
    const formBody = await formResponse.text();
    expect([200, 201], formBody).toContain(formResponse.status());
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
    const sqlRelation = form?.sql_relation;
    expect(sqlRelation).toBeTruthy();
    const nameColumn = form?.fields?.Merchant?.sql_column;
    const amountColumn = form?.fields?.Amount?.sql_column;
    expect(nameColumn).toBeTruthy();
    expect(amountColumn).toBeTruthy();

    for (const row of [
      { amount: 0, merchant: "Independent row 000" },
      { amount: 1, merchant: "Independent row 001" },
    ]) {
      const entry = await request.post(
        getBackendUrl(`/spaces/${spaceId}/entries`),
        {
          data: {
            form: formName,
            fields: { Merchant: row.merchant, Amount: row.amount },
          },
        },
      );
      expect(entry.status()).toBe(201);
    }

    const dependentSql = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: {
          name: dependentSqlName,
          kind: "user-query",
          sql: `SELECT COUNT(*) AS matching_count, MAX(${sqlIdentifier(amountColumn!)}) AS maximum_amount FROM ${
            sqlIdentifier(sqlRelation!)
          } WHERE ${sqlIdentifier(amountColumn!)} >= $minimum`,
          variables: [{
            type: "integer",
            name: "minimum",
            description: "Minimum amount included in the count.",
          }],
        },
      },
    );
    expect([200, 201]).toContain(dependentSql.status());
    const independentSql = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: {
          name: independentSqlName,
          kind: "user-query",
          sql: `SELECT ${sqlIdentifier(nameColumn!)} AS merchant FROM ${
            sqlIdentifier(sqlRelation!)
          } WHERE ${sqlIdentifier(amountColumn!)} >= $minimum ORDER BY ${
            sqlIdentifier(amountColumn!)
          } ASC`,
          variables: [{
            type: "integer",
            name: "minimum",
            description: "Minimum amount included in the independent table.",
          }],
        },
      },
    );
    expect([200, 201]).toContain(independentSql.status());

    type EvaluationExchange = {
      evaluationId: string;
      request: Record<string, unknown>;
      response?: Record<string, unknown>;
      failure?: string;
    };
    const evaluationExchanges = new Map<string, EvaluationExchange[]>();
    const exchangeByRequest = new WeakMap<Request, EvaluationExchange>();
    page.on("request", (outgoing) => {
      if (
        new URL(outgoing.url()).pathname.endsWith("/compositions/evaluate")
      ) {
        let body: Record<string, unknown> = {};
        try {
          body = outgoing.postDataJSON() as Record<string, unknown>;
        } catch {
          // Keep malformed requests in the trace so the matching assertions
          // below fail instead of silently omitting them.
        }
        const exchange = {
          evaluationId: typeof body.evaluation_id === "string"
            ? body.evaluation_id
            : "",
          request: body,
        };
        const exchangesForId = evaluationExchanges.get(exchange.evaluationId) ??
          [];
        exchangesForId.push(exchange);
        evaluationExchanges.set(exchange.evaluationId, exchangesForId);
        exchangeByRequest.set(outgoing, exchange);
      }
    });
    page.on("response", async (incoming) => {
      if (
        new URL(incoming.url()).pathname.endsWith("/compositions/evaluate")
      ) {
        const exchange = exchangeByRequest.get(incoming.request());
        if (!exchange) return;
        try {
          exchange.response = await incoming.json() as Record<string, unknown>;
        } catch {
          exchange.response = {};
        }
      }
    });
    page.on("requestfailed", (outgoing) => {
      const exchange = exchangeByRequest.get(outgoing);
      if (exchange) {
        exchange.failure = outgoing.failure()?.errorText ?? "request failed";
      }
    });

    await page.goto(
      getFrontendUrl(`/spaces/${spaceId}/compositions/new`),
      { waitUntil: "domcontentloaded" },
    );
    await page.getByLabel("Name", { exact: true }).fill(
      `Design dependency preview ${suffix}`,
    );
    await addStudioTypedInput(page, "minimum", "integer", "0");
    await addStudioTypedInput(page, "independent_minimum", "integer", "0");
    await addStudioInput(page, "minimum");
    await addStudioInput(page, "independent_minimum");
    await addMetricDisplay(page, dependentSqlName, "matching_count", {
      minimum: "minimum",
    });
    await addCanvasDisplay(
      page,
      "Table",
      independentSqlName,
      undefined,
      { minimum: "independent_minimum" },
    );

    const dependentVariableId = `${
      savedSqlVariablePrefix(dependentSqlName)
    }_matching_count`;
    const independentVariableId = savedSqlVariablePrefix(independentSqlName);
    const independentTableVariableId = `${independentVariableId}_table`;
    const dependentMetric = page.locator(
      '.compositionGraphMetric[aria-label="matching_count"] strong',
    );
    const independentTable = page.getByRole("table", {
      name: `${independentTableVariableId} results`,
    });
    await expect(dependentMetric).toHaveText("2", { timeout: 30_000 });
    await expect(independentTable).toContainText("Independent row 000");
    await expect(page.getByRole("textbox", {
      name: "independent_minimum",
      exact: true,
    })).toHaveValue("0");
    const successfulEvaluationFor = (variableId: string) =>
      [...evaluationExchanges.entries()].flatMap(([evaluationId, exchanges]) =>
        exchanges.filter((exchange) => {
          const requested = exchange.request.requested_queries;
          const computed = exchange.response?.computed_query_ids;
          return exchange.evaluationId === evaluationId &&
            exchange.failure === undefined &&
            Array.isArray(requested) && requested.includes(variableId) &&
            exchange.response?.ok === true &&
            exchange.response.evaluation_id === evaluationId &&
            Array.isArray(computed) && computed.includes(variableId);
        })
      )[0];
    await expect.poll(() =>
      Boolean(
        successfulEvaluationFor(dependentVariableId) &&
          successfulEvaluationFor(independentTableVariableId),
      )
    ).toBe(true);
    const initialDependentExchange =
      successfulEvaluationFor(dependentVariableId)!;
    const initialTableExchange =
      successfulEvaluationFor(independentTableVariableId)!;
    const initialDependentEvaluation = initialDependentExchange.response!;
    const initialEvaluation = initialTableExchange.response!;
    expect(initialDependentExchange.evaluationId).toBeTruthy();
    expect(initialTableExchange.evaluationId).toBeTruthy();
    expect([...evaluationExchanges.keys()].every(Boolean)).toBe(true);
    expect(initialDependentEvaluation.evaluation_id).toBe(
      initialDependentExchange.evaluationId,
    );
    expect(initialDependentEvaluation.computed_query_ids).toContain(
      dependentVariableId,
    );
    expect(initialDependentEvaluation.query_results).toHaveProperty(
      dependentVariableId,
    );
    expect(initialDependentEvaluation.publication_fingerprint).toBe(
      initialEvaluation.publication_fingerprint,
    );
    expect(initialDependentEvaluation.authorization_fingerprint).toBe(
      initialEvaluation.authorization_fingerprint,
    );
    expect(initialEvaluation.evaluation_id).toBe(
      initialTableExchange.evaluationId,
    );
    expect(initialEvaluation.computed_query_ids).toContain(
      independentTableVariableId,
    );
    expect(initialEvaluation.publication_fingerprint).toBeTruthy();
    expect(initialEvaluation.authorization_fingerprint).toBeTruthy();
    expect(initialEvaluation.query_results).toHaveProperty(
      independentTableVariableId,
    );

    await expect(independentTable.locator("tbody tr")).toHaveCount(2);
    await expect(independentTable).toContainText("Independent row 001");
    const independentRowsBefore = await independentTable.locator("tbody tr")
      .allTextContents();
    const evaluationIdsBeforeInputChange = new Set(
      evaluationExchanges.keys(),
    );

    await page.getByRole("textbox", { name: "minimum", exact: true }).fill("1");
    await expect(dependentMetric).toHaveText("1", { timeout: 30_000 });
    const changedInputAttempts = () =>
      [...evaluationExchanges.entries()]
        .filter(([evaluationId]) =>
          !evaluationIdsBeforeInputChange.has(evaluationId)
        )
        .flatMap(([, exchanges]) => exchanges)
        .filter((exchange) =>
          exchange.request.variables !== null &&
          typeof exchange.request.variables === "object" &&
          (exchange.request.variables as Record<string, unknown>).minimum ===
            1 &&
          JSON.stringify(exchange.request.requested_queries) ===
            JSON.stringify([dependentVariableId]) &&
          !Object.hasOwn(exchange.request, "page")
        );
    await expect.poll(() => {
      const attempts = changedInputAttempts();
      return attempts.length > 0 &&
        attempts.every((exchange) =>
          exchange.response !== undefined || exchange.failure !== undefined
        );
    }).toBe(true);
    const changedInputExchange = changedInputAttempts().find((exchange) =>
      exchange.failure === undefined && exchange.response?.ok === true &&
        exchange.response.evaluation_id === exchange.evaluationId &&
        Array.isArray(exchange.response.computed_query_ids) &&
        exchange.response.computed_query_ids.includes(dependentVariableId)
    );
    expect(changedInputExchange).toBeDefined();
    expect(changedInputExchange!.evaluationId).toBeTruthy();
    expect(changedInputExchange!.request.target).toEqual(
      initialTableExchange.request.target,
    );
    const changedInputEvaluation = changedInputExchange!.response!;
    expect(changedInputEvaluation.evaluation_id).toBe(
      changedInputExchange!.evaluationId,
    );
    for (
      const superseded of changedInputAttempts().filter((exchange) =>
        exchange.failure !== undefined
      )
    ) {
      expect(superseded.response).toBeUndefined();
      expect(superseded.evaluationId).toBeTruthy();
    }
    expect(changedInputEvaluation.computed_query_ids).toEqual([
      dependentVariableId,
    ]);
    expect(changedInputEvaluation.query_results).not.toHaveProperty(
      independentTableVariableId,
    );
    expect(changedInputEvaluation.publication_fingerprint).toBe(
      initialEvaluation.publication_fingerprint,
    );
    expect(changedInputEvaluation.authorization_fingerprint).toBe(
      initialEvaluation.authorization_fingerprint,
    );
    await expect.poll(() =>
      independentTable.locator("tbody tr").allTextContents()
    ).toEqual(independentRowsBefore);
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
        palette.getByRole("button", { name: "Table", exact: true }),
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

test("Composition Studio text remains readable when the system color mode changes", async ({ browser, context, request }) => {
  await waitForServers(request);
  const spaceId = await getDefaultSpaceId(request);
  const storageState = await context.storageState();
  const themeContext = await browser.newContext({
    storageState,
    viewport: { width: 390, height: 844 },
    colorScheme: "light",
  });
  try {
    const page = await themeContext.newPage();
    await page.goto(
      getFrontendUrl(`/spaces/${spaceId}/compositions/new`),
      { waitUntil: "domcontentloaded" },
    );
    await expect(page.locator("html")).toHaveAttribute(
      "data-color-mode",
      "light",
    );

    const palette = page.getByRole("group", { name: "Add to design" });
    await palette.getByRole("button", { name: "Text", exact: true })
      .click();
    await page.getByRole("button", { name: "Edit text", exact: true })
      .click();
    const text = page.getByRole("textbox", { name: "Text", exact: true });
    await expect(text).toBeVisible();
    await text.fill("Daily report");

    for (const colorScheme of ["light", "dark", "light"] as const) {
      await page.emulateMedia({ colorScheme });
      await expect(page.locator("html")).toHaveAttribute(
        "data-color-mode",
        colorScheme,
      );
      await expect(text).toBeVisible();
      await expect(text).toHaveValue("Daily report");

      const contrastRatio = await text.evaluate((element) => {
        const parseRgb = (color: string): [number, number, number] => {
          const channels = color.match(/[\d.]+/g)?.slice(0, 3).map(Number);
          if (!channels || channels.length !== 3) {
            throw new Error(`Expected a computed RGB color, got ${color}`);
          }
          return channels as [number, number, number];
        };
        const luminance = (color: string): number => {
          const channels = parseRgb(color).map((channel) => {
            const value = channel / 255;
            return value <= 0.04045
              ? value / 12.92
              : ((value + 0.055) / 1.055) ** 2.4;
          });
          return 0.2126 * channels[0] + 0.7152 * channels[1] +
            0.0722 * channels[2];
        };
        const style = getComputedStyle(element);
        const foreground = luminance(style.color);
        const background = luminance(style.backgroundColor);
        return (Math.max(foreground, background) + 0.05) /
          (Math.min(foreground, background) + 0.05);
      });
      expect(contrastRatio).toBeGreaterThanOrEqual(4.5);
    }
  } finally {
    await themeContext.close();
  }
});

test("Composition shared Arguments rename and type editing persist explicit bindings", async ({ page, request }) => {
  test.setTimeout(120_000);
  await waitForServers(request);
  const spaceId = await getDefaultSpaceId(request);
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
    components: [{ id: "result", kind: "metric", variable: "rows" }],
    rows: [{
      id: "results",
      items: [{ kind: "component", component: "result" }],
    }],
  });
  const saved = await saveCompositionGraph(request, spaceId, yaml);
  await page.goto(
    getFrontendUrl(
      `/spaces/${spaceId}/compositions/${saved.composition_id}/${saved.revision_id}/edit`,
    ),
    { waitUntil: "domcontentloaded" },
  );
  await expect(page.locator(".compositionGraphMetric strong")).toHaveText("3", {
    timeout: 30_000,
  });
  await page.getByRole("tab", { name: "Variables", exact: true }).click();
  await page.getByRole("button", { name: "rows", exact: true }).click();
  await page.getByRole("button", { name: "Arguments", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Arguments", exact: true });
  await dialog.getByLabel("Argument name for limit", { exact: true }).fill(
    "minimum",
  );
  await dialog.getByRole("button", { name: "Rename argument", exact: true })
    .click();
  await expect(dialog.getByLabel("Argument name for minimum", { exact: true }))
    .toHaveValue("minimum");
  await expect(
    dialog.getByRole("button", { name: "Remove minimum", exact: true }),
  ).toBeDisabled();
  await dialog.getByRole("combobox", { name: "Type for minimum", exact: true })
    .focus();
  await dialog.getByRole("combobox", { name: "Type for minimum", exact: true })
    .selectOption(
      "string",
    );
  await expect(dialog).toContainText("minimum");
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  const binding = page.getByRole("combobox", { name: "minimum", exact: true });
  await expect(binding).toHaveValue("");
  await expect(page.getByRole("button", { name: "Save", exact: true }))
    .toBeDisabled();
  await binding.selectOption("caption");
  await page.getByRole("button", { name: "Arguments", exact: true }).click();
  await dialog.getByRole("combobox", { name: "Type for minimum", exact: true })
    .focus();
  await dialog.getByRole("combobox", { name: "Type for minimum", exact: true })
    .selectOption(
      "integer",
    );
  await expect(dialog).toContainText("minimum");
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(binding).toHaveValue("");
  await binding.selectOption("threshold");
  await expect(page.getByRole("button", { name: "Save", exact: true }))
    .toBeEnabled();
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page).toHaveURL(
    new RegExp(`/compositions/${saved.composition_id}/[^/]+$`),
    { timeout: 30_000 },
  );
  await expect(page.locator(".compositionGraphMetric strong")).toHaveText("3", {
    timeout: 30_000,
  });
  const revisionUrl = new URL(page.url()).pathname;
  await page.goto(getFrontendUrl(revisionUrl + "/edit"), {
    waitUntil: "domcontentloaded",
  });
  await page.getByRole("tab", { name: "Variables", exact: true }).click();
  await page.getByRole("button", { name: "rows", exact: true }).click();
  await expect(page.getByRole("combobox", { name: "minimum", exact: true }))
    .toHaveValue("threshold");
  await expect(page.locator(".cm-content")).toContainText(
    "SELECT $minimum AS value -- $limit stays in a comment",
  );
});

test("unnamed mobile Composition keeps Text focus and inserts existing data before naming", async ({ page, request }) => {
  test.setTimeout(120_000);
  page.setDefaultTimeout(15_000);
  await waitForServers(request);
  const spaceId = await getDefaultSpaceId(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `MobileReceipt${suffix}`;
  const form = await request.post(getBackendUrl(`/spaces/${spaceId}/forms`), {
    data: {
      name: formName,
      version: 1,
      template: `# ${formName}\n\n## Name\n`,
      fields: { Name: { type: "string", required: true } },
    },
  });
  expect([200, 201]).toContain(form.status());
  const entry = await request.post(
    getBackendUrl(`/spaces/${spaceId}/entries`),
    {
      data: { form: formName, fields: { Name: `Mobile row ${suffix}` } },
    },
  );
  expect(entry.status()).toBe(201);
  const sqlName = `Mobile scalar ${suffix}`;
  const sql = await request.post(getBackendUrl(`/spaces/${spaceId}/sql`), {
    data: {
      name: sqlName,
      kind: "user-query",
      sql: "SELECT 42 AS value",
      variables: [],
    },
  });
  expect([200, 201]).toContain(sql.status());
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(getFrontendUrl(`/spaces/${spaceId}/compositions/new`), {
    waitUntil: "domcontentloaded",
  });
  const palette = page.getByRole("group", {
    name: "Add to design",
    exact: true,
  });
  const name = page.getByRole("textbox", { name: "Name", exact: true });
  const save = page.getByRole("button", { name: "Save", exact: true });
  await expect(name).toHaveValue("");
  await palette.getByRole("button", { name: "Text", exact: true }).click();
  await page.locator(".compositionGraphEmptyText").click();
  const text = page.getByRole("textbox", { name: "Text", exact: true });
  const original = await text.elementHandle();
  expect(original).toBeTruthy();
  const scale = await page.evaluate(() => window.visualViewport?.scale ?? 1);
  expect(
    await text.evaluate((element) =>
      Number.parseFloat(getComputedStyle(element).fontSize)
    ),
  ).toBeGreaterThanOrEqual(16);
  await text.focus();
  await text.pressSequentially("Daily report");
  await expect(text).toHaveValue("Daily report");
  const textStyle = page.getByRole("combobox", {
    name: "Text style",
    exact: true,
  });
  await expect(textStyle).toHaveValue("body");
  await textStyle.selectOption("heading");
  await expect(textStyle).toHaveValue("heading");
  expect(
    await original!.evaluate((element) =>
      element.isConnected && element === document.activeElement
    ),
  ).toBe(true);
  expect(
    await text.evaluate((element) =>
      (element as HTMLTextAreaElement).selectionStart
    ),
  ).toBe("Daily report".length);
  expect(await page.evaluate(() => window.visualViewport?.scale ?? 1)).toBe(
    scale,
  );
  await page.getByRole("complementary", {
    name: "Selected component",
    exact: true,
  })
    .getByRole("button", { name: "Close", exact: true }).click();
  await palette.getByRole("button", { name: "Table", exact: true }).click();
  const table = page.getByRole("dialog", { name: "Table", exact: true });
  await table.getByRole("tab", { name: "Forms", exact: true }).click();
  await table.getByRole("button", { name: formName, exact: true }).click();
  await expect(table).toHaveCount(0);
  await expect(
    page.getByRole("cell", { name: `Mobile row ${suffix}`, exact: true }),
  ).toBeVisible({ timeout: 30_000 });
  await palette.getByRole("button", { name: "Metric", exact: true }).click();
  const metric = page.getByRole("dialog", { name: "Metric", exact: true });
  await metric.getByRole("tab", { name: "Forms", exact: true }).click();
  await metric.getByRole("button", { name: formName, exact: true }).click();
  await metric.getByRole("combobox", { name: "Aggregation", exact: true })
    .selectOption("count");
  await metric.getByRole("button", { name: "Add", exact: true }).click();
  await expect(metric).toHaveCount(0);
  await expect(page.locator(".compositionGraphMetric strong")).toHaveText(
    ["1"],
    { timeout: 30_000 },
  );
  await palette.getByRole("button", { name: "Metric", exact: true }).click();
  await metric.getByRole("tab", { name: "Saved SQL", exact: true }).click();
  await metric.getByRole("button", { name: sqlName, exact: true }).click();
  await expect(metric).toHaveCount(0, { timeout: 30_000 });
  await expect(page.locator(".compositionGraphMetric strong")).toHaveText([
    "1",
    "42",
  ], { timeout: 30_000 });
  await expect(name).toHaveValue("");
  await expect(save).toBeDisabled();
  expect(
    await page.evaluate(() =>
      document.documentElement.scrollWidth <=
        document.documentElement.clientWidth
    ),
  ).toBe(true);
  await page.screenshot({
    path: test.info().outputPath("unnamed-mobile-live-data.png"),
    fullPage: true,
  });
  await name.fill(`Mobile report ${suffix}`);
  await expect(save).toBeEnabled();
  await save.click();
  await expect(page).toHaveURL(/\/compositions\/[^/]+\/[^/]+$/, {
    timeout: 30_000,
  });
  const savedRevision = revisionFromUrl(page.url());
  const savedResponse = await request.get(
    getBackendUrl(
      `/spaces/${spaceId}/compositions/${savedRevision.compositionId}/history/${savedRevision.revisionId}`,
    ),
  );
  expect(savedResponse.ok()).toBe(true);
  const savedEntry = await savedResponse.json() as StudioRawRevision;
  const savedDocument = parseYaml(
    String(savedEntry.fields["spec"]),
  ) as {
    spec?: { components?: Array<{ kind?: string; style?: string }> };
  };
  expect(savedDocument.spec?.components?.find((component) =>
    component.kind === "text"
  )?.style).toBe("heading");
  await page.reload({ waitUntil: "domcontentloaded" });
  const reopenedText = page.getByRole("heading", {
    name: "Daily report",
    exact: true,
  });
  await expect(reopenedText).toBeVisible({ timeout: 30_000 });
  await expect(reopenedText).toHaveClass(/compositionGraphText--heading/);
  await expect(page.locator(".compositionGraphMetric strong")).toHaveText([
    "1",
    "42",
  ], { timeout: 30_000 });
});

test(
  "Composition Form choice keeps keyboard focus through document revalidation at narrow widths",
  async ({ page, request }, testInfo) => {
    test.setTimeout(120_000);
    page.setDefaultTimeout(15_000);
    await waitForServers(request);
    const spaceId = await getDefaultSpaceId(request);
    const formName = `FocusForm${crypto.randomUUID().slice(0, 8)}`;
    const created = await request.post(
      getBackendUrl(`/spaces/${spaceId}/forms`),
      {
        data: {
          name: formName,
          version: 1,
          template: `# ${formName}\n`,
          fields: {},
        },
      },
    );
    expect([200, 201]).toContain(created.status());
    const yaml = await completeCompositionGraphYaml(request, spaceId, {
      name: `Form focus ${formName}`,
      variables: [{
        id: "value",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: {
          kind: "inline_sql",
          sql: "SELECT 1",
          arguments: [],
          form_bindings: [],
          schema_fingerprint: "",
        },
      }],
      components: [{ id: "metric", kind: "metric", variable: "value" }],
      rows: [{
        id: "row",
        items: [{ kind: "component", component: "metric" }],
      }],
    });
    const saved = await saveCompositionGraph(request, spaceId, yaml);
    await page.goto(
      getFrontendUrl(
        `/spaces/${spaceId}/compositions/${saved.composition_id}/${saved.revision_id}/edit`,
      ),
    );
    await page.getByRole("tab", { name: "Variables", exact: true }).click();
    await page.getByRole("button", { name: "value", exact: true }).click();
    const opener = page.getByRole("button", { name: "Form", exact: true });
    for (const width of [320, 375, 390]) {
      await page.setViewportSize({ width, height: 844 });
      await opener.click();
      const dialog = page.getByRole("dialog", { name: "Form", exact: true });
      const choice = dialog.getByRole("button", {
        name: formName,
        exact: true,
      });
      await expect(choice).toBeEnabled();
      await expect(dialog.getByRole("tree")).toHaveCount(0);
      const size = await choice.boundingBox();
      expect(size?.height).toBeGreaterThanOrEqual(44);
      expect(
        await page.evaluate(() =>
          document.documentElement.scrollWidth <= innerWidth
        ),
      ).toBe(true);
      if (width === 390) {
        await page.screenshot({
          path: testInfo.outputPath("sql-form-picker-390.png"),
          fullPage: true,
        });
        await choice.click();
        await expect(page.locator(".ui-sql-editor")).toContainText(
          `SELECT 1 FROM "${formName}"`,
        );
      } else {
        await page.keyboard.press("Escape");
      }
      await expect(dialog).toHaveCount(0);
      await expect(opener).toBeFocused();
    }
  },
);
