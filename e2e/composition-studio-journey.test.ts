import {
  type APIRequestContext,
  type Browser,
  type BrowserContext,
  expect,
  type Page,
  test,
} from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";

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
  label?: string,
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
  if (label) {
    await page.getByLabel("Label", { exact: true }).fill(label);
  }
}

async function addMetricDisplay(
  page: Page,
  sourceName: string,
  column: string,
  label: string,
): Promise<void> {
  await addCanvasDisplay(page, "Metric", sourceName, column, label);
}

async function addStudioDateVariable(
  page: Page,
  label: string,
  defaultValue: string,
): Promise<void> {
  await page.getByLabel("New variable label", { exact: true }).fill(label);
  await page.getByLabel("Type", { exact: true }).selectOption("date");
  await page.getByRole("button", { name: "Add variable", exact: true }).click();
  const defaultInput = page.getByLabel(`Default for ${label}`, {
    exact: true,
  });
  await expect(defaultInput).toBeVisible();
  await defaultInput.fill(defaultValue);
}

async function addStudioStringVariable(
  page: Page,
  label: string,
  defaultValue: string,
): Promise<void> {
  await page.getByLabel("New variable label", { exact: true }).fill(label);
  await page.getByLabel("Type", { exact: true }).selectOption("string");
  await page.getByRole("button", { name: "Add variable", exact: true }).click();
  const defaultInput = page.getByLabel(`Default for ${label}`, {
    exact: true,
  });
  await expect(defaultInput).toBeVisible();
  await defaultInput.fill(defaultValue);
}

async function addStudioIntegerVariable(
  page: Page,
  label: string,
  defaultValue: string,
): Promise<void> {
  await page.getByLabel("New variable label", { exact: true }).fill(label);
  await page.getByLabel("Type", { exact: true }).selectOption("integer");
  await page.getByRole("button", { name: "Add variable", exact: true }).click();
  const defaultInput = page.getByLabel(`Default for ${label}`, {
    exact: true,
  });
  await expect(defaultInput).toBeVisible();
  await defaultInput.fill(defaultValue);
}

async function bindSavedSqlVariable(
  page: Page,
  sqlVariable: string,
  compositionVariable: string,
): Promise<void> {
  const binding = page.getByLabel(`SQL variable ${sqlVariable}`, {
    exact: true,
  });
  await expect(binding).toBeVisible();
  await binding.selectOption({ label: compositionVariable });
  await expect(binding.locator("option:checked")).toHaveText(
    compositionVariable,
  );
}

async function addStudioInput(
  page: Page,
  variableName: string,
  inputLabel: string,
): Promise<void> {
  await page.getByRole("radio", { name: "Design", exact: true }).click();
  await page.getByRole("button", { name: "Add to design", exact: true })
    .first().click();
  const palette = page.getByRole("dialog", {
    name: "Add to design",
    exact: true,
  });
  await palette.getByRole("button", {
    name: `Add Input for ${variableName}`,
    exact: true,
  }).click();
  // Adding an Input selects it and opens its inspector automatically.
  await page.getByLabel(`Input label for ${variableName}`, {
    exact: true,
  }).fill(inputLabel);
}

async function addStudioText(page: Page, variableLabel: string): Promise<void> {
  await page.getByRole("radio", { name: "Design", exact: true }).click();
  await page.getByRole("button", { name: "Add to design", exact: true })
    .first().click();
  const palette = page.getByRole("dialog", {
    name: "Add to design",
    exact: true,
  });
  await palette.getByRole("button", { name: "Text", exact: true }).click();
  await page.getByLabel("Content", { exact: true }).selectOption({
    label: variableLabel,
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
    "tags: []",
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
    "      expected_result:",
    "        - name: total",
    "          type: float",
    "        - name: quantity",
    "          type: float",
    "      variables:",
    "        month_start:",
    "          variable: month_start",
    "        month_end:",
    "          variable: month_end",
    "  components:",
    "    - id: total",
    "      kind: metric",
    "      label: Total",
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

      // Declare typed Composition variables and bind the Saved SQL names
      // through their visible labels. Values from the standalone SQL run are
      // deliberately authored again as defaults in the Composition.
      await page.getByRole("radio", { name: "Data", exact: true }).click();
      const dataTabs = page.getByRole("tablist", { name: "Data" });
      await dataTabs.getByRole("tab", { name: "Variables", exact: true })
        .click();
      await addStudioDateVariable(page, "Month start", seed.monthStart);
      await addStudioDateVariable(page, "Month end", seed.monthEnd);
      await addStudioStringVariable(page, "Report title", "January expenses");

      await dataTabs.getByRole("tab", { name: "Sources", exact: true })
        .click();
      await page.getByRole("button", {
        name: seed.savedSqlName,
        exact: true,
      }).click();
      await bindSavedSqlVariable(page, "month_start", "Month start");
      await bindSavedSqlVariable(page, "month_end", "Month end");

      // Inputs are ordinary layout components. The labels users see at run
      // time are authored separately from the SQL variable names.
      await addStudioInput(page, "Month start", "Start date");
      await addStudioInput(page, "Month end", "End date");
      await addStudioInput(page, "Report title", "Report title");
      await addStudioText(page, "Report title");

      // Two metric displays on the seeded Saved SQL source, each bound to
      // its own exact-scalar column of the single January row.
      await addMetricDisplay(page, seed.savedSqlName, "total", "Total");
      await addMetricDisplay(page, seed.savedSqlName, "quantity", "Quantity");
      await expect(
        page.getByRole("button", { name: "Select Total", exact: true }),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Select Quantity", exact: true }),
      ).toBeVisible();

      // EntryQuery source through the Data source navigator.
      await page.getByRole("radio", { name: "Split", exact: true }).click();
      await page.getByRole("button", { name: "Add source", exact: true })
        .click();
      const sourceDialog = page.getByRole("dialog");
      await expect(sourceDialog).toBeVisible();
      await sourceDialog.getByRole("button", {
        name: seed.formName,
        exact: true,
      }).click();
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await expect(
        page.getByRole("button", { name: seed.formName, exact: true }),
      ).toHaveCount(1);

      // Metric sources are Saved SQL only; the Form remains available for
      // Tables through the same source catalog.
      await page.getByRole("button", { name: "Add to design", exact: true })
        .first().click();
      const palette = page.getByRole("dialog", {
        name: "Add to design",
        exact: true,
      });
      await palette.getByRole("button", { name: "Data", exact: true })
        .click();
      const metricPicker = page.getByRole("dialog", {
        name: "Add data component",
        exact: true,
      });
      const kindTabs = metricPicker.getByRole("tablist", {
        name: "Data component type",
      });
      await kindTabs.getByRole("tab", { name: "Metric", exact: true }).click();
      await expect(metricPicker.getByRole("button", {
        name: seed.formName,
        exact: true,
      })).toHaveCount(0);
      await expect(metricPicker.getByRole("button", {
        name: seed.savedSqlName,
        exact: true,
      })).toBeVisible();
      await metricPicker.getByRole("button", { name: "Cancel", exact: true })
        .click();

      // Table display on the EntryQuery source through the canvas.
      await addCanvasDisplay(page, "Table", seed.formName);
      await expect(
        page.getByRole("button", { name: seed.formName, exact: true }),
      ).toHaveCount(1);
      await expect(
        page.getByRole("button", {
          name: `Select ${seed.formName}`,
          exact: true,
        }),
      ).toBeVisible();

      // Data mode keeps Sources, Variables, and Tags in their own tabs.
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
      await expect(page.getByLabel("Default for Month start")).toHaveValue(
        seed.monthStart,
      );
      await expect(page.getByLabel("Default for Month end")).toHaveValue(
        seed.monthEnd,
      );
      await expect(page.getByLabel("Default for Report title")).toHaveValue(
        "January expenses",
      );
      await expect(page.getByLabel("Default for month_start")).toHaveCount(0);
      await expect(page.getByLabel("Default for month_end")).toHaveCount(0);
      await dataTabs.getByRole("tab", { name: "Tags", exact: true }).click();
      await expect(
        page.getByRole("textbox", { name: "Tags", exact: true }),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: seed.savedSqlName, exact: true }),
      ).toBeHidden();
      await page.getByRole("radio", { name: "Design", exact: true }).click();
      await expect(
        page.getByRole("heading", { name: "Total", exact: true }),
      ).toBeVisible({ timeout: 30_000 });
      await expect(
        page.getByRole("heading", { name: "Quantity", exact: true }),
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
      // Scope to the EntryQuery table: the canvas also renders the
      // Saved SQL table, so the bare table role is ambiguous.
      const previewTable = page.locator("table.result-table--entry");
      for (const purpose of seed.purposes) {
        await expect(previewTable).toContainText(purpose, { timeout: 30_000 });
      }

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
      const declaredVariableId = (label: string) =>
        savedYaml.match(
          new RegExp(`- id: ([^\\n]+)\\n\\s+label: ${label}\\n`),
        )?.[1];
      const boundVariableId = (sqlName: string) =>
        savedYaml.match(
          new RegExp(`${sqlName}:\\s*\\n\\s+variable:\\s*([^\\n]+)`),
        )?.[1];
      const monthStartVariable = declaredVariableId("Month start");
      const monthStartBinding = boundVariableId("month_start");
      expect(monthStartVariable).toBeTruthy();
      expect(monthStartBinding).toBeTruthy();
      expect(monthStartBinding).toBe(monthStartVariable);
      const monthEndVariable = declaredVariableId("Month end");
      const monthEndBinding = boundVariableId("month_end");
      expect(monthEndVariable).toBeTruthy();
      expect(monthEndBinding).toBeTruthy();
      expect(monthEndBinding).toBe(monthEndVariable);
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
      await expect(page.getByLabel("Start date")).toHaveValue(
        seed.monthStart,
      );
      await expect(page.getByLabel("Report title")).toHaveValue(
        "January expenses",
      );
      await expect(page.getByText("January expenses", { exact: true }))
        .toBeVisible();
      await expect(page.getByLabel("Start date")).toBeEditable();
      await expect(page.getByLabel("End date")).toHaveValue(seed.monthEnd);
      await expect(page.getByLabel("End date")).toBeEditable();
      await expect(page.getByLabel("month_start")).toHaveCount(0);
      await expect(page.getByLabel("month_end")).toHaveCount(0);
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
      const savedTable = page.locator("table.result-table--entry");
      for (const purpose of seed.purposes) {
        await expect(savedTable).toContainText(purpose, { timeout: 30_000 });
      }

      // A Text-only variable update is synchronous and must not re-execute the
      // Saved SQL Table or Metric source.
      const initialSqlQueryCount = sqlQueryCount;
      const initialMetricQueryCount = metricQueryCount;
      expect(initialSqlQueryCount).toBeGreaterThan(0);
      expect(initialMetricQueryCount).toBeGreaterThan(0);
      await page.getByLabel("Report title", { exact: true }).fill(
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
      await expect(page.getByLabel("Report title")).toHaveValue(
        "January expenses",
      );
      await expect(page.getByText("January expenses", { exact: true }))
        .toBeVisible();
      await expectHistoryTotal(request, seed.spaceId, compositionId, 1);

      // Edit the exact revision: relabel one canvas block, save a new revision.
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
        page.getByRole("button", { name: "Select Start date", exact: true }),
      ).toBeVisible();
      await page.getByRole("button", {
        name: "Select Start date",
        exact: true,
      }).click();
      const inputVariable = page.getByLabel("Variables", { exact: true });
      await expect(inputVariable.locator("option:checked")).toHaveText(
        "Month start",
      );
      await expect(page.getByLabel("Input label for Month start"))
        .toHaveValue("Start date");
      await page.getByRole("button", { name: "Select Total", exact: true })
        .click();
      await page.getByLabel("Label", { exact: true }).fill("Total spent");
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
        page.getByRole("heading", { name: "Total spent", exact: true }),
      ).toBeVisible();
      await expect(page.getByLabel("Start date")).toHaveValue(seed.monthStart);
      await expect(page.getByLabel("End date")).toHaveValue(seed.monthEnd);

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
        page.getByRole("heading", { name: "Total", exact: true }),
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
        page.getByRole("heading", { name: "Total", exact: true }),
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
      await addStudioIntegerVariable(page, "Minimum amount", "0");

      await dataTabs.getByRole("tab", { name: "Sources", exact: true })
        .click();
      await page.getByRole("button", { name: seed.savedSqlName, exact: true })
        .click();
      await bindSavedSqlVariable(page, "minimum", "Minimum amount");

      await addStudioInput(page, "Minimum amount", "Minimum amount");
      await addMetricDisplay(
        page,
        seed.savedSqlName,
        "matching_count",
        "Matching expenses",
      );
      await addMetricDisplay(
        page,
        seed.savedSqlName,
        "total_amount",
        "Total amount",
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
      const matchingExpenses = page.locator("section.section").filter({
        has: page.getByRole("heading", {
          name: "Matching expenses",
          exact: true,
        }),
      }).locator("output.compositionMetric");
      const totalAmount = page.locator("section.section").filter({
        has: page.getByRole("heading", { name: "Total amount", exact: true }),
      }).locator("output.compositionMetric");

      await expect(page.getByLabel("Minimum amount", { exact: true }))
        .toHaveValue("0");
      await expect(matchingExpenses).toHaveText("3", { timeout: 30_000 });
      await expect(totalAmount).toHaveText("60", { timeout: 30_000 });
      const initialQueryCount = metricQueryCount;
      expect(initialQueryCount).toBeGreaterThan(0);

      await page.getByLabel("Minimum amount", { exact: true }).fill("20");
      await expect(matchingExpenses).toHaveText("2", { timeout: 30_000 });
      await expect(totalAmount).toHaveText("50", { timeout: 30_000 });
      await expect.poll(() => metricQueryCount).toBeGreaterThan(
        initialQueryCount,
      );

      // Input state is transient: reload returns to the saved default and does
      // not append a Composition revision.
      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(page.getByLabel("Minimum amount", { exact: true }))
        .toHaveValue("0");
      await expect(matchingExpenses).toHaveText("3", { timeout: 30_000 });
      await expect(totalAmount).toHaveText("60", { timeout: 30_000 });
      await expectHistoryTotal(request, seed.spaceId, compositionId, 1);
    } finally {
      await journeyContext.close();
    }
  });

  test("Composition Studio inspector stays usable at a 390px viewport", async ({ browser, context, request }) => {
    // Story 5 (mobile Inspector), assertion-only: the seeded studio opens at
    // 390px, a block tap renders the existing inspector content in the
    // bottom sheet, Escape dismisses it, and Data mode shows the source.
    // Seeded through the API so this leg never depends on the build test.
    test.setTimeout(120_000);
    const storageState = await context.storageState();
    const suffix = crypto.randomUUID().slice(0, 8);
    const compositionName = `Studio mobile ${suffix}`;
    const saveResponse = await request.post(
      getBackendUrl(`/spaces/${seed.spaceId}/compositions`),
      {
        headers: { "Idempotency-Key": crypto.randomUUID() },
        data: {
          yaml: buildMobileStudioYaml(
            compositionName,
            seed.savedSqlId,
            seed.savedSqlRevisionId,
            seed.monthStart,
            seed.monthEnd,
          ),
        },
      },
    );
    expect([200, 201]).toContain(saveResponse.status());
    const saved = await saveResponse.json() as {
      composition_id?: string;
      revision_id?: string;
    };
    expect(saved.composition_id).toBeTruthy();
    expect(saved.revision_id).toBeTruthy();

    const mobileContext = await browser.newContext({
      storageState,
      viewport: { width: 390, height: 844 },
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
        name: "Select Total",
        exact: true,
      });
      await expect(selectButton).toBeVisible({ timeout: 30_000 });
      await selectButton.click();
      // Below the sheet gate the tap opens the inspector as a bottom sheet
      // reusing the wide-viewport content: same dialog name, same Label
      // field.
      const sheet = page.getByRole("dialog", { name: "Metric" });
      await expect(sheet).toBeVisible();
      await expect(sheet.getByLabel("Label", { exact: true })).toHaveValue(
        "Total",
      );
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
      await expect(
        page.getByRole("heading", { name: "Data", exact: true }),
      ).toBeVisible();
      const dataTabs = page.getByRole("tablist", { name: "Data" });
      await page.getByRole("button", { name: "Show data sources", exact: true })
        .click();
      await expect(
        page.getByRole("button", { name: seed.savedSqlName, exact: true }),
      ).toBeVisible();
      await dataTabs.getByRole("tab", { name: "Variables", exact: true })
        .click();
      await expect(page.getByLabel("Default for Variable 1")).toHaveValue(
        seed.monthStart,
      );
      await expect(page.getByLabel("Default for month_start")).toHaveCount(0);
    } finally {
      await mobileContext.close();
    }
  });
});
