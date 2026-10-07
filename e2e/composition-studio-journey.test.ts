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
  expectedCount: string;
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
  const purposes = ["Studio Jan one", "Studio Jan two", "Studio Feb one"];
  const entries = [
    { date: "2026-01-05", purpose: purposes[0], amount: 10 },
    { date: "2026-01-15", purpose: purposes[1], amount: 20 },
    { date: "2026-02-05", purpose: purposes[2], amount: 30 },
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
`,
        fields: {
          Date: { type: "date", required: true },
          Purpose: { type: "string", required: true },
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
  expect(dateColumn).toBeTruthy();
  expect(purposeColumn).toBeTruthy();
  expect(amountColumn).toBeTruthy();

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
          },
        },
      },
    );
    expect(entryResponse.status()).toBe(201);
  }

  const sql = `SELECT SUM(${
    sqlIdentifier(amountColumn!)
  }) AS total, COUNT(*) AS count FROM ${
    sqlIdentifier(form!.sql_relation!)
  } WHERE ${sqlIdentifier(dateColumn!)} >= $month_start AND ${
    sqlIdentifier(dateColumn!)
  } < $month_end`;
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
    expectedTotal: "30",
    expectedCount: "2",
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
  // palette metric/table entries delegate to the display picker at the
  // recorded target. The legacy Display section is gone.
  await page.getByRole("button", { name: "Add block", exact: true }).first()
    .click();
  const palette = page.getByRole("dialog", {
    name: "Add block",
    exact: true,
  });
  await expect(palette).toBeVisible();
  await palette.getByRole("button", { name: kind, exact: true }).click();
  const dialog = page.getByRole("dialog", {
    name: "Add display",
    exact: true,
  });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: kind, exact: true }).click();
  await dialog.getByRole("button", { name: sourceName, exact: true }).click();
  if (kind === "Metric") {
    await dialog.getByLabel("Value", { exact: true }).selectOption(value!);
    await dialog.getByLabel("Label", { exact: true }).fill(label!);
  }
  await dialog.getByRole("button", { name: "Add display", exact: true })
    .click();
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
      await expect(runTable).toContainText(seed.expectedTotal);

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

      // Two metric displays on the seeded Saved SQL source.
      await addMetricDisplay(page, seed.savedSqlName, "total", "Total");
      await addMetricDisplay(page, seed.savedSqlName, "count", "Count");
      await expect(
        page.getByRole("button", { name: "Select Total", exact: true }),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Select Count", exact: true }),
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

      // Parameter defaults edit in Data mode, where Parameters live, then
      // the Design canvas carries the display assertions.
      await page.getByRole("radio", { name: "Data", exact: true }).click();
      await page.getByLabel("Default for month_start").fill(seed.monthStart);
      await page.getByLabel("Default for month_end").fill(seed.monthEnd);
      await page.getByRole("radio", { name: "Design", exact: true }).click();
      await expect(
        page.getByRole("heading", { name: "Total", exact: true }),
      ).toBeVisible();
      await expect(
        page.getByRole("heading", { name: "Count", exact: true }),
      ).toBeVisible();
      await expect(
        page.locator("output.compositionMetric").filter({
          hasText: new RegExp(`^${seed.expectedTotal}$`),
        }),
      ).toBeVisible();
      await expect(
        page.locator("output.compositionMetric").filter({
          hasText: new RegExp(`^${seed.expectedCount}$`),
        }),
      ).toBeVisible();
      const previewTable = page.getByRole("table");
      for (const purpose of seed.purposes) {
        await expect(previewTable).toContainText(purpose);
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
      ).toBeVisible();
      await expect(
        page.locator("output.compositionMetric").filter({
          hasText: new RegExp(`^${seed.expectedCount}$`),
        }),
      ).toBeVisible();
      const savedTable = page.getByRole("table");
      for (const purpose of seed.purposes) {
        await expect(savedTable).toContainText(purpose);
      }

      // Edit the exact revision: relabel one canvas block, save a new revision.
      await page.goto(
        getFrontendUrl(
          `/spaces/${seed.spaceId}/compositions/${compositionId}/${firstRevisionId}/edit`,
        ),
        { waitUntil: "domcontentloaded" },
      );
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
      ).toBeVisible();
      await page.getByRole("button", {
        name: "Restore this revision",
        exact: true,
      }).click();
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
      ).toBeVisible();
      await expect(
        page.getByText("Total spent", { exact: true }),
      ).toHaveCount(0);
    } finally {
      await journeyContext.close();
    }
  });
});
