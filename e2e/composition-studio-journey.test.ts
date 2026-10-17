import {
  type APIRequestContext,
  type Browser,
  type BrowserContext,
  expect,
  type Page,
  test,
} from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";
import { completeSavedSqlCompositionYaml } from "./lib/composition.ts";

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

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

async function createSpace(request: APIRequestContext): Promise<string> {
  const suffix = `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
  const response = await request.post(getBackendUrl("/spaces"), {
    data: {
      slug: `composition-studio-${suffix}`,
      name: `Composition Studio ${suffix}`,
    },
  });
  expect([200, 201]).toContain(response.status());
  const result = await response.json() as { space_uid?: string };
  expect(result.space_uid).toBeTruthy();
  return result.space_uid!;
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
): Promise<void> {
  // A gap opens the design palette; its Data entry opens the shared
  // Table/Metric source picker at the recorded target.
  await page.getByRole("button", { name: "Add to design", exact: true }).first()
    .click();
  const palette = page.getByRole("dialog", {
    name: "Add to design",
    exact: true,
  });
  await expect(palette).toBeVisible();
  await palette.getByRole("button", { name: "Data", exact: true }).click();
  const dialog = page.getByRole("dialog", {
    name: "Add data component",
    exact: true,
  });
  await expect(dialog).toBeVisible();
  const dialogWidth = Number.parseFloat(
    await dialog.evaluate((element) => getComputedStyle(element).width),
  );
  expect(dialogWidth).toBeGreaterThan(600);
  const kindTabs = dialog.getByRole("tablist", {
    name: "Data component type",
  });
  await kindTabs.getByRole("tab", { name: kind, exact: true }).click();
  const source = dialog.getByRole("button", { name: sourceName, exact: true });
  await source.click();
  if (kind === "Table") {
    await expect(page.getByRole("dialog")).toHaveCount(0);
    return;
  }
  await expect(source).toHaveAttribute("aria-pressed", "true");
  await dialog.getByLabel("Column", { exact: true }).selectOption(column!);
  await dialog.getByRole("button", { name: "Preview", exact: true })
    .click();
  await expect(dialog.locator("output.compositionMetricPreview"))
    .toBeVisible();
  await dialog.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
}

async function addMetricDisplay(
  page: Page,
  sourceName: string,
  column: string,
): Promise<void> {
  await addCanvasDisplay(page, "Metric", sourceName, column);
}

async function setStudioVariableDefault(
  page: Page,
  variableId: string,
  defaultValue: string,
): Promise<void> {
  const defaultInput = page.getByLabel(`Default for ${variableId}`, {
    exact: true,
  });
  await expect(defaultInput).toBeVisible();
  await expect(defaultInput).toHaveValue("");
  await defaultInput.fill(defaultValue);
}

async function addStudioStringVariable(
  page: Page,
  variableId: string,
  defaultValue: string,
): Promise<void> {
  await page.getByLabel("Variable ID", { exact: true }).fill(variableId);
  await page.getByLabel("Type", { exact: true }).last().selectOption("string");
  await page.getByRole("button", { name: "Add variable", exact: true }).click();
  const defaultInput = page.getByLabel(`Default for ${variableId}`, {
    exact: true,
  });
  await expect(defaultInput).toBeVisible();
  await defaultInput.fill(defaultValue);
}

async function addStudioInput(
  page: Page,
  variableId: string,
): Promise<void> {
  await page.getByRole("radio", { name: "Design", exact: true }).click();
  await page.getByRole("button", { name: "Add to design", exact: true })
    .first().click();
  const palette = page.getByRole("dialog", {
    name: "Add to design",
    exact: true,
  });
  await palette.getByRole("button", {
    name: `Add Input for ${variableId}`,
    exact: true,
  }).click();
}

async function addStudioText(page: Page, variableId: string): Promise<void> {
  await page.getByRole("radio", { name: "Design", exact: true }).click();
  await page.getByRole("button", { name: "Add to design", exact: true })
    .first().click();
  const palette = page.getByRole("dialog", {
    name: "Add to design",
    exact: true,
  });
  await palette.getByRole("button", { name: "Text", exact: true }).click();
  await page.getByRole("button", { name: "Select Text", exact: true }).click();
  await page.getByLabel("Content", { exact: true }).selectOption({
    label: variableId,
  });
}

async function openToolFromHome(
  page: Page,
  spaceId: string,
  compositionId: string,
  revisionId: string,
  toolName: string,
): Promise<string> {
  const path = `/spaces/${spaceId}/compositions/${compositionId}/${revisionId}`;
  await page.goto(getFrontendUrl(`/spaces/${spaceId}/dashboard`), {
    waitUntil: "domcontentloaded",
  });
  const link = page.getByRole("link", {
    name: new RegExp(`^${toolName} Dashboard`),
  });
  await expect(link).toHaveAttribute("href", path);
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

function buildMobileStudioYaml(
  compositionName: string,
  savedSqlId: string,
  savedSqlRevisionId: string,
  monthStart: string,
  monthEnd: string,
): string {
  // Same single-row source as the seeded Saved SQL: one Metric on the amount
  // column plus a table, with the month range carried as variable defaults
  // so the tool resolves without caller input.
  return [
    "format: ugoite.composition",
    "format_version: 1",
    `name: ${compositionName}`,
    "kind: dashboard",
    "spec:",
    "  variables:",
    "    - id: month_start",
    "      type: date",
    "      required: true",
    `      default: ${monthStart}`,
    "    - id: month_end",
    "      type: date",
    "      required: true",
    `      default: ${monthEnd}`,
    "  sources:",
    "    - id: studio_rows",
    "      kind: saved_sql",
    `      entry_id: "${savedSqlId}"`,
    `      revision_id: "${savedSqlRevisionId}"`,
    "      expected_result: []",
    "      schema_fingerprint: ''",
    "      variables:",
    "        month_start:",
    "          variable: month_start",
    "        month_end:",
    "          variable: month_end",
    "  components:",
    "    - id: total",
    "      kind: metric",
    "      source: studio_rows",
    "      column: total",
    "    - id: rows",
    "      kind: table",
    "      source: studio_rows",
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
      );
      const runTable = page.getByRole("table");
      await expect(runTable.locator("tbody tr")).toHaveCount(1);
      await expect(runTable).toContainText(seed.expectedTotal, {
        timeout: 30_000,
      });

      // Studio opens with the pinned Saved SQL source, ready for explicit
      // Composition-variable declarations and bindings.
      await page.getByRole("button", { name: "Save as tool" }).click();
      await expect(page).toHaveURL(
        new RegExp(
          `/spaces/${seed.spaceId}/compositions/new$`,
        ),
      );
      const nameInput = page.getByLabel("Name", { exact: true });
      await expect(nameInput).toHaveValue(seed.savedSqlName);
      await expect(
        page.getByRole("button", {
          name: `Select ${seed.savedSqlName}`,
          exact: true,
        }),
      ).toBeVisible();
      // Studio modes: Split pairs Design with Data for the seeded source.
      await page.getByRole("radio", { name: "Split", exact: true }).click();
      await expect(
        page.getByRole("button", { name: seed.savedSqlName, exact: true }),
      ).toBeVisible();
      await nameInput.fill(seed.toolName);

      // The pinned SQL schema declares typed Composition variables and places
      // required Inputs. Copying run values into Composition defaults remains
      // an explicit authoring action.
      await page.getByRole("radio", { name: "Data", exact: true }).click();
      const dataTabs = page.getByRole("tablist", { name: "Data" });
      await dataTabs.getByRole("tab", { name: "Variables", exact: true })
        .click();
      await setStudioVariableDefault(page, "month_start", seed.monthStart);
      await setStudioVariableDefault(page, "month_end", seed.monthEnd);
      await addStudioStringVariable(page, "report_title", "January expenses");

      await dataTabs.getByRole("tab", { name: "Sources", exact: true })
        .click();
      await page.getByRole("button", {
        name: seed.savedSqlName,
        exact: true,
      }).click();

      // SQL parameter Inputs were placed with the seeded typed variables.
      await addStudioInput(page, "report_title");
      await addStudioText(page, "report_title");

      // Two metric displays on the seeded Saved SQL source, each bound to
      // its own exact-scalar column of the single January row.
      await addMetricDisplay(page, seed.savedSqlName, "total");
      await addMetricDisplay(page, seed.savedSqlName, "quantity");
      await expect(
        page.getByRole("button", {
          name: `Select ${seed.savedSqlName} / total`,
          exact: true,
        }),
      ).toBeVisible();
      await expect(
        page.getByRole("button", {
          name: `Select ${seed.savedSqlName} / quantity`,
          exact: true,
        }),
      ).toBeVisible();

      // Data mode keeps Sources and Variables in their own tabs.
      await page.getByRole("radio", { name: "Data", exact: true }).click();
      await expect(
        dataTabs.getByRole("tab", { name: "Sources", exact: true }),
      ).toHaveAttribute("aria-selected", "true");
      await expect(
        page.getByRole("button", { name: seed.savedSqlName, exact: true }),
      ).toBeVisible();
      await test.info().attach("composition-studio-data-sources", {
        body: await page.screenshot({ fullPage: true }),
        contentType: "image/png",
      });
      await dataTabs.getByRole("tab", { name: "Variables", exact: true })
        .click();
      await test.info().attach("composition-studio-data-variables", {
        body: await page.screenshot({ fullPage: true }),
        contentType: "image/png",
      });
      await expect(page.getByLabel("Default for month_start")).toHaveValue(
        seed.monthStart,
      );
      await expect(page.getByLabel("Default for month_end")).toHaveValue(
        seed.monthEnd,
      );
      await expect(page.getByLabel("Default for report_title")).toHaveValue(
        "January expenses",
      );
      await page.getByRole("radio", { name: "Design", exact: true }).click();
      await expect(
        page.getByRole("heading", { name: "total", exact: true }),
      ).toBeVisible({ timeout: 30_000 });
      await expect(
        page.getByRole("heading", { name: "quantity", exact: true }),
      ).toBeVisible({ timeout: 30_000 });
      await expect(
        page.locator("output.compositionMetric").filter({
          hasText: new RegExp(`^${seed.expectedTotal}$`),
        }),
      ).toBeVisible({ timeout: 30_000 });
      await expect(
        page.locator("output.compositionMetric").filter({
          hasText: new RegExp(`^${seed.expectedQuantity}$`),
        }),
      ).toBeVisible({ timeout: 30_000 });
      const previewTable = page.locator("table.result-table--sql");
      await expect(previewTable).toContainText(seed.expectedTotal, {
        timeout: 30_000,
      });
      await expect(previewTable).toContainText(seed.expectedQuantity, {
        timeout: 30_000,
      });

      // Save once: exactly one publication.
      await page.getByRole("button", { name: "Save", exact: true }).click();
      await expect(page).toHaveURL(
        new RegExp(
          `/spaces/${seed.spaceId}/compositions/[^/]+/[^/]+$`,
        ),
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
      const boundVariableId = (sqlName: string) =>
        savedYaml.match(
          new RegExp(`${sqlName}:\\s*\\n\\s+variable:\\s*([^\\n]+)`),
        )?.[1];
      const monthStartBinding = boundVariableId("month_start");
      expect(monthStartBinding).toBeTruthy();
      expect(monthStartBinding).toBe("month_start");
      const monthEndBinding = boundVariableId("month_end");
      expect(monthEndBinding).toBeTruthy();
      expect(monthEndBinding).toBe("month_end");
    } finally {
      await buildContext.close();
    }

    const journeyContext = await openOwnerContext(browser, storageState);
    try {
      const page = await journeyContext.newPage();
      let sqlQueryCount = 0;
      let metricQueryCount = 0;
      page.on("request", (request) => {
        const pathname = new URL(request.url()).pathname;
        if (
          pathname.endsWith("/sql/query") ||
          pathname.endsWith("/sql/query/count")
        ) sqlQueryCount += 1;
        if (pathname.endsWith("/compositions/metric-query")) {
          metricQueryCount += 1;
        }
      });
      // Home rediscovers the tool; the exact revision keeps its values.
      await openToolFromHome(
        page,
        seed.spaceId,
        compositionId,
        firstRevisionId,
        seed.toolName,
      );
      await expect(page.getByLabel("month_start")).toHaveValue(
        seed.monthStart,
      );
      await expect(page.getByLabel("report_title")).toHaveValue(
        "January expenses",
      );
      await expect(page.getByText("January expenses", { exact: true }))
        .toBeVisible();
      await expect(page.getByLabel("month_start")).toBeEditable();
      await expect(page.getByLabel("month_end")).toHaveValue(seed.monthEnd);
      await expect(page.getByLabel("month_end")).toBeEditable();
      await expect(
        page.locator("output.compositionMetric").filter({
          hasText: new RegExp(`^${seed.expectedTotal}$`),
        }),
      ).toBeVisible({ timeout: 30_000 });
      await expect(
        page.locator("output.compositionMetric").filter({
          hasText: new RegExp(`^${seed.expectedQuantity}$`),
        }),
      ).toBeVisible({ timeout: 30_000 });
      const savedTable = page.locator("table.result-table--sql");
      await expect(savedTable).toContainText(seed.expectedTotal, {
        timeout: 30_000,
      });
      await expect(savedTable).toContainText(seed.expectedQuantity, {
        timeout: 30_000,
      });

      // A Text-only variable update is synchronous and must not re-execute the
      // Saved SQL Table or Metric source.
      const initialSqlQueryCount = sqlQueryCount;
      const initialMetricQueryCount = metricQueryCount;
      expect(initialSqlQueryCount).toBeGreaterThan(0);
      expect(initialMetricQueryCount).toBeGreaterThan(0);
      await page.getByLabel("report_title", { exact: true }).fill(
        "Transient report title",
      );
      await expect(
        page.getByText("Transient report title", { exact: true }),
      ).toBeVisible();
      await page.waitForTimeout(500);
      expect(sqlQueryCount).toBe(initialSqlQueryCount);
      expect(metricQueryCount).toBe(initialMetricQueryCount);

      // Runtime edits live only in Work: a fresh load of the saved revision
      // restores the declared default and does not publish another revision.
      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(page.getByLabel("report_title")).toHaveValue(
        "January expenses",
      );
      await expect(page.getByText("January expenses", { exact: true }))
        .toBeVisible();
      await expectHistoryTotal(request, seed.spaceId, compositionId, 1);

      // Edit the exact revision by changing a metric's SQL result column.
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
      await expect(
        page.getByRole("button", { name: "Select month_start", exact: true }),
      ).toBeVisible();
      await page.getByRole("button", {
        name: "Select month_start",
        exact: true,
      }).click();
      const inputVariable = page.getByLabel("Variables", { exact: true });
      await expect(inputVariable.locator("option:checked")).toHaveText(
        "month_start",
      );
      await page.getByRole("button", {
        name: `Select ${seed.savedSqlName} / total`,
        exact: true,
      }).click();
      await expect(page.getByLabel("Column", { exact: true })).toHaveValue(
        "total",
      );
      await page.getByLabel("Column", { exact: true }).selectOption("quantity");
      await expect(page.getByLabel("Label", { exact: true })).toHaveCount(0);
      await page.getByRole("button", { name: "Save", exact: true }).click();
      await expect(page).toHaveURL(
        new RegExp(
          `/spaces/${seed.spaceId}/compositions/${compositionId}/[^/]+$`,
        ),
      );
      const edited = revisionFromUrl(page.url());
      expect(edited.compositionId).toBe(compositionId);
      expect(edited.revisionId).not.toBe(firstRevisionId);
      editedRevisionId = edited.revisionId;
      await expectHistoryTotal(request, seed.spaceId, compositionId, 2);
      await expect(
        page.getByRole("heading", { name: "quantity", exact: true }),
      ).toHaveCount(2);
      await expect(
        page.getByRole("heading", { name: "total", exact: true }),
      ).toHaveCount(0);
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
      );
      const sqlResult = page.getByRole("table");
      await expect(sqlResult.locator("tbody tr")).toHaveCount(1);
      await expect(sqlResult).toContainText("3", { timeout: 30_000 });
      await expect(sqlResult).toContainText("60", { timeout: 30_000 });

      await page.getByRole("button", { name: "Save as tool" }).click();
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${seed.spaceId}/compositions/new$`),
      );
      await page.getByLabel("Name", { exact: true }).fill(seed.toolName);
      await page.getByRole("radio", { name: "Data", exact: true }).click();
      const dataTabs = page.getByRole("tablist", { name: "Data" });
      await dataTabs.getByRole("tab", { name: "Variables", exact: true })
        .click();
      await setStudioVariableDefault(page, "minimum", "0");

      await dataTabs.getByRole("tab", { name: "Sources", exact: true })
        .click();
      await page.getByRole("button", { name: seed.savedSqlName, exact: true })
        .click();
      await page.getByRole("radio", { name: "Design", exact: true }).click();

      await addMetricDisplay(
        page,
        seed.savedSqlName,
        "matching_count",
      );
      await addMetricDisplay(
        page,
        seed.savedSqlName,
        "total_amount",
      );
      await page.getByRole("button", { name: "Save", exact: true }).click();
      await expect(page).toHaveURL(
        new RegExp(
          `/spaces/${seed.spaceId}/compositions/[^/]+/[^/]+$`,
        ),
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
    } finally {
      await buildContext.close();
    }

    const journeyContext = await openOwnerContext(browser, storageState);
    try {
      const page = await journeyContext.newPage();
      let metricQueryCount = 0;
      page.on("request", (request) => {
        if (
          new URL(request.url()).pathname.endsWith("/compositions/metric-query")
        ) {
          metricQueryCount += 1;
        }
      });

      await openToolFromHome(
        page,
        seed.spaceId,
        compositionId,
        revisionId,
        seed.toolName,
      );
      const metrics = page.locator(
        ".compositionFlowItem--metric output.compositionMetric",
      );
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
      const initialQueryCount = metricQueryCount;
      expect(initialQueryCount).toBeGreaterThan(0);

      await page.getByLabel("minimum", { exact: true }).fill("20");
      await expect.poll(metricValues, { timeout: 30_000 }).toEqual([
        "2",
        "50",
      ]);
      await expect.poll(() => metricQueryCount).toBeGreaterThan(
        initialQueryCount,
      );

      // Input state is transient: reload returns to the saved default and does
      // not append a Composition revision.
      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(page.getByLabel("minimum", { exact: true }))
        .toHaveValue("0");
      await expect.poll(metricValues, { timeout: 30_000 }).toEqual([
        "3",
        "60",
      ]);
      await expectHistoryTotal(request, seed.spaceId, compositionId, 1);
    } finally {
      await journeyContext.close();
    }
  });

  test("Composition Studio responsive modes fit target viewports and support keyboard and touch", async ({ browser, context, request }) => {
    // Assertion-only: a seeded block opens in the 390px inspector sheet,
    // Escape dismisses it, source selection works by touch, and Design/Data/
    // Split fit their supported widths with keyboard focus in both color modes.
    test.setTimeout(120_000);
    const storageState = await context.storageState();
    const suffix = crypto.randomUUID().slice(0, 8);
    const compositionName = `Studio mobile ${suffix}`;
    const yaml = await completeSavedSqlCompositionYaml(
      request,
      seed.spaceId,
      buildMobileStudioYaml(
        compositionName,
        seed.savedSqlId,
        seed.savedSqlRevisionId,
        seed.monthStart,
        seed.monthEnd,
      ),
      [{
        id: "studio_rows",
        savedSqlId: seed.savedSqlId,
        savedSqlRevisionId: seed.savedSqlRevisionId,
      }],
    );
    const saveResponse = await request.post(
      getBackendUrl(`/spaces/${seed.spaceId}/compositions`),
      {
        headers: { "Idempotency-Key": crypto.randomUUID() },
        data: { yaml },
      },
    );
    const saveBody = await saveResponse.text();
    expect([200, 201]).toContain(saveResponse.status());
    const saved = JSON.parse(saveBody) as {
      composition_id?: string;
      revision_id?: string;
    };
    expect(saved.composition_id).toBeTruthy();
    expect(saved.revision_id).toBeTruthy();

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
      // The Design canvas owns the seeded metric block at 390px.
      const selectButton = page.getByRole("button", {
        name: `Select ${seed.savedSqlName} / total`,
        exact: true,
      });
      await expect(selectButton).toBeVisible({ timeout: 30_000 });
      await selectButton.click();
      // Below the sheet gate the tap opens the inspector as a bottom sheet
      // reusing the wide-viewport content with the selected SQL column.
      const sheet = page.getByRole("dialog", { name: "Metric" });
      await expect(sheet).toBeVisible();
      await expect(sheet.getByLabel("Column", { exact: true })).toHaveValue(
        "total",
      );
      await expect(sheet.getByLabel("Label", { exact: true })).toHaveCount(0);
      // Escape dismisses the sheet and clears the transient selection.
      await page.keyboard.press("Escape");
      await expect(page.getByRole("dialog")).toHaveCount(0);

      // The unified picker stays viewport-capped at 390px and keeps a
      // compatible source selected as the type changes.
      await page.getByRole("button", { name: "Add to design", exact: true })
        .first().click();
      const palette = page.getByRole("dialog", {
        name: "Add to design",
        exact: true,
      });
      await palette.getByRole("button", { name: "Data", exact: true })
        .click();
      const picker = page.getByRole("dialog", {
        name: "Add data component",
        exact: true,
      });
      await expect(picker).toBeVisible();
      const viewportWidth = await page.evaluate(() => window.innerWidth);
      const pickerWidth = Number.parseFloat(
        await picker.evaluate((element) => getComputedStyle(element).width),
      );
      expect(pickerWidth).toBeLessThanOrEqual(viewportWidth - 32);
      const displayTabs = picker.getByRole("tablist", {
        name: "Data component type",
      });
      const tableTab = displayTabs.getByRole("tab", { name: "Table" });
      await expect(tableTab).toHaveAttribute("aria-selected", "true");
      const metricTab = displayTabs.getByRole("tab", { name: "Metric" });
      await metricTab.click();
      const source = picker.getByRole("button", {
        name: seed.savedSqlName,
        exact: true,
      });
      await source.click();
      await expect(source).toHaveAttribute("aria-pressed", "true");
      await expect(metricTab).toHaveAttribute("aria-selected", "true");
      await expect(picker.getByLabel("Column", { exact: true })).toBeVisible();
      await picker.getByRole("button", { name: "Cancel", exact: true })
        .click();
      await expect(page.getByRole("dialog")).toHaveCount(0);

      // Data mode shows the seeded source in the Data workspace.
      await page.getByRole("radio", { name: "Data", exact: true }).click();
      await expect(page.getByRole("radio", { name: "Data", exact: true }))
        .toHaveAttribute("aria-checked", "true");
      await expect(
        page.getByRole("heading", { name: "Data", exact: true }),
      ).toHaveCount(0);
      const dataTabs = page.getByRole("tablist", { name: "Data" });
      const showSources = page.getByRole("button", {
        name: /Show data sources/,
      });
      const sourceDrawer = page.getByRole("dialog", { name: "Sources" });
      await expect(sourceDrawer).toBeVisible();
      const sourceItems = sourceDrawer.getByRole("listitem");
      await expect(sourceItems).toHaveCount(1);
      const sourceRow = sourceItems.first().getByRole("button", {
        name: seed.savedSqlName,
        exact: true,
      });
      await expect(sourceRow).toHaveText(seed.savedSqlName);
      await expect(sourceRow.locator("svg")).toHaveCount(0);
      const sourceRowHeight = await sourceRow.evaluate((element) =>
        element.getBoundingClientRect().height
      );
      expect(sourceRowHeight).toBeGreaterThanOrEqual(44);
      await sourceRow.tap();
      await expect(sourceDrawer).toHaveCount(0);
      const sourceMenuHeight = await showSources.evaluate((element) =>
        element.getBoundingClientRect().height
      );
      expect(sourceMenuHeight).toBeGreaterThanOrEqual(44);
      await showSources.tap();
      await expect(sourceDrawer).toBeVisible();

      const addSource = sourceDrawer.getByRole("button", {
        name: "Add source",
        exact: true,
      });
      const addSourceHeight = await addSource.evaluate((element) =>
        element.getBoundingClientRect().height
      );
      expect(addSourceHeight).toBeGreaterThanOrEqual(44);
      await addSource.tap();
      const sourcePicker = page.getByRole("dialog", {
        name: "Choose a source",
      });
      await expect(sourcePicker).toBeVisible();
      await expect(page.getByRole("dialog")).toHaveCount(1);
      await expect(page.getByRole("dialog", { name: "Sources" }))
        .toHaveCount(0);
      await expect(page.locator("#app")).toHaveAttribute("inert", "");
      await page.keyboard.press("Escape");
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await expect(page.locator("#app")).not.toHaveAttribute("inert", "");
      await expect(page.getByRole("button", { name: /Show data sources/ }))
        .toBeFocused();
      await dataTabs.getByRole("tab", { name: "Variables", exact: true })
        .click();
      const defaultInput = page.getByLabel("Default for month_start");
      await expect(defaultInput).toHaveValue(seed.monthStart);
      const inputFontSize = Number.parseFloat(
        await defaultInput.evaluate((element) => getComputedStyle(element).fontSize),
      );
      expect(inputFontSize).toBeGreaterThanOrEqual(16);

      // 320px represents the effective content width at 200% browser zoom;
      // actual browser zoom and iPhone Safari remain manual device evidence.
      // Exercise every Studio arrangement at each responsive breakpoint.
      for (const width of [320, 375, 390, 760, 1280]) {
        await page.setViewportSize({ width, height: 844 });
        const modes = width > 760
          ? ["Design", "Data", "Split"]
          : ["Design", "Data"];
        for (const mode of modes) {
          const option = page.getByRole("radio", { name: mode, exact: true });
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
          expect(focusStyle.outlineStyle).toBe("solid");
          expect(focusStyle.outlineWidth).toBeGreaterThanOrEqual(2);
          const pageWidth = await page.evaluate(() => ({
            client: document.documentElement.clientWidth,
            content: document.documentElement.scrollWidth,
          }));
          expect(pageWidth.content).toBeLessThanOrEqual(pageWidth.client);
          const modeOptions = page.getByRole("radiogroup", {
            name: "Studio mode",
          })
            .getByRole("radio");
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

      const surfaceColors = new Map<string, string>();
      for (const colorScheme of ["light", "dark"] as const) {
        await page.emulateMedia({ colorScheme });
        await expect.poll(async () =>
          page.locator("html").getAttribute("data-color-mode")
        ).toBe(colorScheme);
        surfaceColors.set(
          colorScheme,
          await page.locator(".studioMode").evaluate((element) =>
            getComputedStyle(element).backgroundColor
          ),
        );
      }
      expect(surfaceColors.get("light")).not.toBe(surfaceColors.get("dark"));
    } finally {
      await mobileContext.close();
    }
  });
});
