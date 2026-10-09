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

type StudioHistoryPage = {
  revisions: Array<{ revision: { revision_id: string } }>;
  total: number;
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
  // Exactly one January row: metric displays need a single exact-scalar row,
  // and stateless SQL pages are aggregate-free by deliberate product design
  // (no SUM/COUNT/AVG), so the seed binds two scalar columns instead of
  // monthly totals. Quantity gives the second metric its own value column.
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

  // Aggregate-free exact-scalar SQL mirroring the metric journey: plain
  // column selects with ORDER BY. The month range keeps parameter-binding
  // coverage; the January window matches exactly one row so both metrics
  // render scalars instead of the multiple-rows diagnostic.
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
  value?: string,
  label?: string,
): Promise<void> {
  // Canvas insertion path: a gap "+" opens the block palette, and the
  // palette Display entry delegates to the picker at the recorded target.
  await page.getByRole("button", { name: "Add block", exact: true }).first()
    .click();
  const palette = page.getByRole("dialog", {
    name: "Add block",
    exact: true,
  });
  await expect(palette).toBeVisible();
  await palette.getByRole("button", { name: "Display", exact: true }).click();
  const dialog = page.getByRole("dialog", {
    name: "Add display",
    exact: true,
  });
  await expect(dialog).toBeVisible();
  const dialogWidth = Number.parseFloat(
    await dialog.evaluate((element) => getComputedStyle(element).width),
  );
  expect(dialogWidth).toBeGreaterThan(600);
  await dialog.getByRole("tab", { name: kind, exact: true }).click();
  const source = dialog.getByRole("button", { name: sourceName, exact: true });
  await source.click();
  await expect(source).toHaveAttribute("aria-pressed", "true");
  if (kind === "Metric") {
    await dialog.getByLabel("Value", { exact: true }).selectOption(value!);
    await dialog.getByLabel("Label", { exact: true }).fill(label!);
  }
  await dialog.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
}

async function addMetricDisplay(
  page: Page,
  sourceName: string,
  value: string,
  label: string,
): Promise<void> {
  await addCanvasDisplay(page, "Metric", sourceName, value, label);
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
  const link = page.getByRole("link", { name: new RegExp(toolName) });
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
  // Same aggregate-free scalar source as the seeded Saved SQL: one metric on
  // the amount column plus a table, with the month range carried as parameter
  // defaults so the tool resolves without caller input.
  return [
    "format: ugoite.composition",
    "format_version: 1",
    `name: ${compositionName}`,
    "kind: dashboard",
    "tags: []",
    "spec:",
    "  parameters:",
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
    "          parameter: month_start",
    "        month_end:",
    "          parameter: month_end",
    "  components:",
    "    - id: total",
    "      kind: metric",
    "      label: Total",
    "      source: studio_rows",
    "      value_field:",
    "        kind: sql_column",
    "        name: total",
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

      // Studio opens prefilled from the Saved SQL run, save-ready with a
      // default Table on the seeded source: the canvas owns the block.
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
      // Studio modes: Split pairs the Design canvas with the Data pane for
      // the seeded source, so the build below covers add, label, params,
      // and preview without leaving the mode.
      await page.getByRole("radio", { name: "Split", exact: true }).click();
      await expect(
        page.getByRole("button", { name: seed.savedSqlName, exact: true }),
      ).toBeVisible();
      await nameInput.fill(seed.toolName);

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

      // EntryQuery source through the Forms picker.
      await page.getByRole("button", { name: "Add data", exact: true }).click();
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

      // Data mode opens on Sources and keeps Parameters and Tags in their own
      // tabs. Capture the novice path before editing parameter defaults.
      await page.getByRole("radio", { name: "Data", exact: true }).click();
      const dataTabs = page.getByRole("tablist", { name: "Data" });
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
      await dataTabs.getByRole("tab", { name: "Parameters", exact: true })
        .click();
      await test.info().attach("composition-studio-data-parameters", {
        body: await page.screenshot({ fullPage: true }),
        contentType: "image/png",
      });
      await page.getByLabel("Default for month_start").fill(seed.monthStart);
      await page.getByLabel("Default for month_end").fill(seed.monthEnd);
      await dataTabs.getByRole("tab", { name: "Tags", exact: true }).click();
      await expect(page.getByLabel("Tags", { exact: true })).toBeVisible();
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
    } finally {
      await buildContext.close();
    }

    const journeyContext = await openOwnerContext(browser, storageState);
    try {
      const page = await journeyContext.newPage();
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
      await expect(page.getByLabel("month_end")).toHaveValue(seed.monthEnd);
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
        page.getByRole("button", { name: "Add block", exact: true }).first(),
      ).toBeVisible({ timeout: 30_000 });
      await expect(page.getByLabel("Name", { exact: true })).toHaveValue(
        seed.toolName,
      );
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
      await expect(selectButton).toBeVisible();
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
      await page.getByRole("button", { name: "Add block", exact: true })
        .first().click();
      const palette = page.getByRole("dialog", {
        name: "Add block",
        exact: true,
      });
      await palette.getByRole("button", { name: "Display", exact: true })
        .click();
      const picker = page.getByRole("dialog", {
        name: "Add display",
        exact: true,
      });
      await expect(picker).toBeVisible();
      const viewportWidth = await page.evaluate(() => window.innerWidth);
      const pickerWidth = Number.parseFloat(
        await picker.evaluate((element) => getComputedStyle(element).width),
      );
      expect(pickerWidth).toBeLessThanOrEqual(viewportWidth - 32);
      const displayTabs = picker.getByRole("tablist", {
        name: "Display type",
      });
      const tableTab = displayTabs.getByRole("tab", { name: "Table" });
      await expect(tableTab).toHaveAttribute("aria-selected", "true");
      const source = picker.getByRole("button", {
        name: seed.savedSqlName,
        exact: true,
      });
      await source.click();
      await expect(source).toHaveAttribute("aria-pressed", "true");
      const metricTab = displayTabs.getByRole("tab", { name: "Metric" });
      await metricTab.click();
      await expect(metricTab).toHaveAttribute("aria-selected", "true");
      await expect(source).toHaveAttribute("aria-pressed", "true");
      await expect(picker.getByLabel("Value", { exact: true })).toBeVisible();
      await picker.getByRole("button", { name: "Cancel", exact: true })
        .click();
      await expect(page.getByRole("dialog")).toHaveCount(0);

      // Data mode shows the seeded source in the Data workspace.
      await page.getByRole("radio", { name: "Data", exact: true }).click();
      await expect(
        page.getByRole("heading", { name: "Data", exact: true }),
      ).toBeVisible();
      const dataTabs = page.getByRole("tablist", { name: "Data" });
      await expect(
        page.getByRole("button", { name: seed.savedSqlName, exact: true }),
      ).toBeVisible();
      await dataTabs.getByRole("tab", { name: "Parameters", exact: true })
        .click();
      await expect(page.getByLabel("Default for month_start")).toHaveValue(
        seed.monthStart,
      );
    } finally {
      await mobileContext.close();
    }
  });
});
