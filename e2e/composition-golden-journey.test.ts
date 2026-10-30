import {
  type APIRequestContext,
  type Browser,
  type BrowserContext,
  expect,
  type Page,
  test,
} from "@playwright/test";
import { readFile } from "node:fs/promises";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";

const SEED_MANIFEST_URL = new URL(
  "./fixtures/composition/space-seed/manifest.json",
  import.meta.url,
);

type FormField = {
  sql_column?: string;
};

type FormSummary = {
  id?: string;
  name?: string;
  sql_relation?: string;
  fields?: Record<string, FormField>;
};

type SeedRow = {
  date: string;
  amount: number;
  merchant: string;
  page: 1 | 2 | "excluded";
};

type SeedState = {
  spaceId: string;
  savedSqlId: string;
  savedSqlRevisionId: string;
  savedSqlName: string;
  rows: SeedRow[];
};

type SeedManifest = {
  schema: string;
  materialization: string;
  model_connection: "disabled";
  composition_name: string;
  page_limit: number;
  parameters: {
    month_start: string;
    month_end: string;
    month_start_after_change: string;
  };
  seed_rows: {
    first_page_count: number;
    first_page_date: string;
    second_page_date: string;
    excluded_date: string;
  };
  saved_sql: {
    name: string;
    sql_template: string;
    variables: Array<{ type: string; name: string; description: string }>;
  };
  expected: {
    first_page_rows: number;
    second_page_rows: number;
    after_month_start_change_rows: number;
    composition_publications: number;
  };
  selectors: string[];
};

type CompositionSaveResponse = {
  composition_id: string;
  revision_id: string;
  canonical_yaml: string;
  receipt: {
    committed_revision_ids: string[];
  };
};

type CompositionHistoryPage = {
  revisions: Array<{ revision: { revision_id: string } }>;
  total: number;
  offset: number;
  limit: number;
  has_more: boolean;
};

async function readSeedManifest(): Promise<SeedManifest> {
  return JSON.parse(
    await readFile(SEED_MANIFEST_URL, "utf8"),
  ) as SeedManifest;
}

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

function savedSqlVariablePrefix(name: string): string {
  const slug = name.normalize("NFKD").toLowerCase().replace(/\p{M}/gu, "")
    .replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "");
  return slug && /^[a-z]/.test(slug) ? slug : slug ? `query_${slug}` : "query";
}

function createSeedRows(manifest: SeedManifest): SeedRow[] {
  const firstPage = Array.from(
    { length: manifest.seed_rows.first_page_count },
    (_, index): SeedRow => ({
      date: manifest.seed_rows.first_page_date,
      amount: index + 1,
      merchant: `Golden-${String(index + 1).padStart(3, "0")}`,
      page: index < manifest.page_limit ? 1 : 2,
    }),
  );
  return [
    ...firstPage,
    {
      date: manifest.seed_rows.second_page_date,
      amount: 101,
      merchant: "Golden-101",
      page: 2,
    },
    {
      date: manifest.seed_rows.excluded_date,
      amount: 102,
      merchant: "Golden-excluded",
      page: "excluded",
    },
  ];
}

async function createSpace(request: APIRequestContext): Promise<string> {
  const suffix = `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
  const response = await request.post(getBackendUrl("/spaces"), {
    data: {
      slug: `composition-golden-${suffix}`,
      name: `Composition Golden ${suffix}`,
    },
  });
  expect([200, 201]).toContain(response.status());
  const result = await response.json() as { space_uid?: string };
  expect(result.space_uid).toBeTruthy();
  return result.space_uid!;
}

async function seedSavedSql(
  request: APIRequestContext,
  manifest: SeedManifest,
): Promise<SeedState> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `MonthlyExpense${suffix}`;

  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}

## Date

## Merchant

## Amount
`,
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
  const forms = await formsResponse.json() as FormSummary[];
  const form = forms.find((candidate) => candidate.name === formName);
  expect(form?.id).toBeTruthy();
  expect(form?.sql_relation).toBeTruthy();
  const dateColumn = form!.fields?.Date?.sql_column;
  const merchantColumn = form!.fields?.Merchant?.sql_column;
  const amountColumn = form!.fields?.Amount?.sql_column;
  expect(dateColumn).toBeTruthy();
  expect(merchantColumn).toBeTruthy();
  expect(amountColumn).toBeTruthy();

  const rows = createSeedRows(manifest);
  for (let offset = 0; offset < rows.length; offset += 8) {
    await Promise.all(
      rows.slice(offset, offset + 8).map(async (row) => {
        const entryResponse = await request.post(
          getBackendUrl(`/spaces/${spaceId}/entries`),
          {
            data: {
              form: formName,
              fields: {
                Date: row.date,
                Merchant: row.merchant,
                Amount: row.amount,
              },
            },
          },
        );
        expect(entryResponse.status(), await entryResponse.text()).toBe(201);
      }),
    );
  }

  const sql = manifest.saved_sql.sql_template
    .replaceAll("{relation}", sqlIdentifier(form!.sql_relation!))
    .replaceAll("{date_column}", sqlIdentifier(dateColumn!))
    .replaceAll("{merchant_column}", sqlIdentifier(merchantColumn!))
    .replaceAll("{amount_column}", sqlIdentifier(amountColumn!));
  const savedSqlName = `${manifest.saved_sql.name} ${suffix}`;
  const sqlResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name: savedSqlName,
        kind: "user-query",
        sql,
        variables: manifest.saved_sql.variables,
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
    savedSqlId: savedSql.id!,
    savedSqlRevisionId: savedSql.revision_id!,
    savedSqlName,
    rows,
  };
}

async function openOwnerContext(
  browser: Browser,
  storageState: Awaited<ReturnType<BrowserContext["storageState"]>>,
) {
  return await browser.newContext({ storageState });
}

function savePath(spaceId: string): string {
  return new URL(
    getBackendUrl(`/spaces/${spaceId}/compositions`),
  ).pathname;
}

async function openSavedCompositionFromHome(
  page: Page,
  spaceId: string,
  compositionId: string,
  revisionId: string,
  compositionName: string,
): Promise<string> {
  const path = `/spaces/${spaceId}/compositions/${compositionId}/${revisionId}`;
  await page.goto(getFrontendUrl(`/spaces/${spaceId}/dashboard`), {
    waitUntil: "domcontentloaded",
  });
  const savedTools = page.getByRole("list", { name: "Saved tools" });
  const link = savedTools.getByRole("link", { name: compositionName });
  await expect(link).toHaveAttribute("href", path);
  await link.click();
  await expect(page).toHaveURL(new RegExp(`${path.replaceAll("/", "\\/")}$`));
  await expect(
    page.getByRole("heading", { level: 1, name: compositionName }),
  ).toBeVisible();
  return path;
}

async function addDateVariable(
  page: Page,
  variableId: string,
  defaultValue: string,
): Promise<void> {
  await page.getByRole("tablist", { name: "Studio mode" })
    .getByRole("tab", { name: "Variables", exact: true }).click();
  await page.getByRole("button", { name: "Add variable", exact: true }).click();
  await page.getByRole("group", { name: "Add variable" })
    .getByRole("button", { name: "Input", exact: true }).click();
  await page.getByLabel("Variable ID", { exact: true }).fill(variableId);
  await page.getByLabel("Type", { exact: true }).selectOption("date");
  await page.getByLabel("Default", { exact: true }).fill(defaultValue);
}

async function addInput(page: Page, variableId: string): Promise<void> {
  await page.getByRole("tablist", { name: "Studio mode" })
    .getByRole("tab", { name: "Design", exact: true }).click();
  await page.getByRole("group", { name: "Add to design" })
    .getByLabel("Input", { exact: true }).selectOption({ label: variableId });
}

test.describe("Composition Golden Journey", () => {
  let seed: SeedState;
  let manifest: SeedManifest;

  test.beforeAll(async ({ request }) => {
    // The 102-row fixture publishes bounded batches; slow hosts need
    // room beyond the default per-hook budget.
    test.setTimeout(300_000);
    await waitForServers(request);
    manifest = await readSeedManifest();
    expect(manifest.model_connection).toBe("disabled");
    expect(manifest.expected.first_page_rows).toBe(manifest.page_limit);
    expect(manifest.expected.composition_publications).toBe(1);
    seed = await seedSavedSql(request, manifest);
  });

  test("Browser saves a tool once, reopens its exact revision from Home, and pages parameterized results with model connection disabled", async ({ browser, context, request }) => {
    test.setTimeout(120_000);
    const storageState = await context.storageState();
    const initialContext = await openOwnerContext(browser, storageState);
    let compositionId = "";
    let revisionId = "";
    let committedSave: CompositionSaveResponse | undefined;
    let replayedSave: CompositionSaveResponse | undefined;
    const idempotencyKeys: string[] = [];
    const requestBodies: unknown[] = [];
    try {
      const initialPage = await initialContext.newPage();
      await initialPage.goto(
        getFrontendUrl(
          `/spaces/${seed.spaceId}/sql/${seed.savedSqlId}/variables`,
        ),
        { waitUntil: "domcontentloaded" },
      );
      await initialPage.getByLabel("month_start").fill(
        manifest.parameters.month_start,
      );
      await initialPage.getByLabel("month_end").fill(
        manifest.parameters.month_end,
      );
      await initialPage.getByRole("button", { name: "Run", exact: true })
        .click();
      await expect(initialPage).toHaveURL(
        new RegExp(
          `/spaces/${seed.spaceId}/sql/${seed.savedSqlId}/run$`,
        ),
      );
      await expect(initialPage.getByRole("table").locator("tbody tr"))
        .toHaveCount(manifest.expected.first_page_rows);

      const saveUrl = savePath(seed.spaceId);
      await initialPage.route("**/api/spaces/*/compositions", async (route) => {
        const routed = new URL(route.request().url());
        if (
          route.request().method() !== "POST" ||
          routed.pathname !== saveUrl
        ) {
          await route.continue();
          return;
        }

        idempotencyKeys.push(
          route.request().headers()["idempotency-key"] ?? "",
        );
        requestBodies.push(route.request().postDataJSON());
        const response = await route.fetch();
        expect(response.ok()).toBe(true);
        const responseBody = await response.body();
        const saved = JSON.parse(
          new TextDecoder().decode(responseBody),
        ) as CompositionSaveResponse;
        if (idempotencyKeys.length === 1) {
          committedSave = saved;
          await new Promise((resolve) => setTimeout(resolve, 500));
          await route.fulfill({
            status: 502,
            contentType: "application/json",
            body: JSON.stringify({
              code: "SAVE_RESPONSE_DELAYED",
              message: "The save response was interrupted after commit.",
            }),
          });
          return;
        }
        replayedSave = saved;
        await route.fulfill({
          status: response.status(),
          headers: response.headers(),
          body: responseBody,
        });
      });

      await initialPage.getByRole("button", { name: "Save as tool" }).click();
      await expect(initialPage).toHaveURL(
        new RegExp(
          `/spaces/${seed.spaceId}/compositions/new(?:\\?.*)?$`,
        ),
      );
      // Save as tool opens the shared binding picker with the exact SQL
      // revision. Declare graph Inputs before binding its named arguments.
      const initialPicker = initialPage.getByRole("dialog", { name: "Table" });
      await expect(initialPicker).toBeVisible();
      await initialPage.keyboard.press("Escape");
      await expect(initialPicker).toHaveCount(0);
      const nameInput = initialPage.getByLabel("Name", { exact: true });
      await expect(nameInput).toHaveValue(seed.savedSqlName);
      await nameInput.fill(manifest.composition_name);
      await addDateVariable(
        initialPage,
        "month_start",
        manifest.parameters.month_start,
      );
      await addDateVariable(
        initialPage,
        "month_end",
        manifest.parameters.month_end,
      );
      // Give the second Table its own start-date dependency. This lets the
      // acceptance check change one query while the first Table is on page 2.
      await addDateVariable(
        initialPage,
        "month_start_other",
        manifest.parameters.month_start,
      );
      await addInput(initialPage, "month_start");
      await addInput(initialPage, "month_end");
      await addInput(initialPage, "month_start_other");
      await initialPage.getByRole("tablist", { name: "Studio mode" })
        .getByRole("tab", { name: "Design", exact: true }).click();
      await initialPage.getByRole("group", { name: "Add to design" })
        .getByRole("button", { name: "Table", exact: true }).click();
      const picker = initialPage.getByRole("dialog", {
        name: "Table",
        exact: true,
      });
      await picker.getByRole("tab", { name: "Saved SQL", exact: true }).click();
      const savedSqlChoice = picker.getByRole("button", {
        name: seed.savedSqlName,
        exact: true,
      });
      await expect(savedSqlChoice).toBeEnabled({ timeout: 30_000 });
      await savedSqlChoice.click();
      for (const argument of ["month_start", "month_end"]) {
        await picker.getByLabel(`Variable for SQL argument ${argument}`, {
          exact: true,
        })
          .selectOption({ label: argument });
      }
      await picker.getByRole("button", { name: "Add", exact: true }).click();
      await expect(picker).toHaveCount(0);
      await initialPage.getByRole("group", { name: "Add to design" })
        .getByRole("button", { name: "Table", exact: true }).click();
      const independentPicker = initialPage.getByRole("dialog", {
        name: "Table",
        exact: true,
      });
      await independentPicker.getByRole("tab", {
        name: "Saved SQL",
        exact: true,
      }).click();
      const independentSqlChoice = independentPicker.getByRole("button", {
        name: seed.savedSqlName,
        exact: true,
      });
      await expect(independentSqlChoice).toBeEnabled({ timeout: 30_000 });
      await independentSqlChoice.click();
      await independentPicker.getByLabel(
        "Variable for SQL argument month_start",
        {
          exact: true,
        },
      ).selectOption({ label: "month_start_other" });
      await independentPicker.getByLabel(
        "Variable for SQL argument month_end",
        {
          exact: true,
        },
      ).selectOption({ label: "month_end" });
      await independentPicker.getByRole("button", { name: "Add", exact: true })
        .click();
      await expect(independentPicker).toHaveCount(0);
      await initialPage.getByRole("button", { name: "Save", exact: true })
        .click();
      await expect(
        initialPage.getByText("Could not save this tool.", { exact: true }),
      ).toBeVisible();

      const replayResponsePromise = initialPage.waitForResponse((response) => {
        const url = new URL(response.url());
        return response.request().method() === "POST" &&
          url.pathname === saveUrl && response.status() >= 200 &&
          response.status() < 300;
      });
      await initialPage.getByRole("button", { name: "Retry", exact: true })
        .click();
      const replayResponse = await replayResponsePromise;
      replayedSave = await replayResponse.json() as CompositionSaveResponse;

      expect(idempotencyKeys).toHaveLength(2);
      expect(idempotencyKeys[0]).toBeTruthy();
      expect(idempotencyKeys[1]).toBe(idempotencyKeys[0]);
      expect(requestBodies[1]).toEqual(requestBodies[0]);
      expect((requestBodies[0] as { yaml?: string }).yaml).toContain(
        seed.savedSqlRevisionId,
      );
      expect(committedSave).toBeTruthy();
      expect(replayedSave).toBeTruthy();
      expect(replayedSave).toEqual(committedSave);

      compositionId = replayedSave!.composition_id;
      revisionId = replayedSave!.revision_id;
      expect(replayedSave!.receipt.committed_revision_ids).toContain(
        revisionId,
      );
      // The Studio save navigates to the exact revision on success.
      await expect(initialPage).toHaveURL(
        new RegExp(
          `/spaces/${seed.spaceId}/compositions/${compositionId}/${revisionId}$`,
        ),
        { timeout: 30_000 },
      );

      const historyResponse = await request.get(
        getBackendUrl(
          `/spaces/${seed.spaceId}/compositions/${compositionId}/history?limit=100&offset=0`,
        ),
      );
      expect(historyResponse.ok()).toBe(true);
      const history = await historyResponse.json() as CompositionHistoryPage;
      expect(history.revisions).toHaveLength(
        manifest.expected.composition_publications,
      );
      expect(history.total).toBe(manifest.expected.composition_publications);
      expect(history.revisions[0]?.revision.revision_id).toBe(revisionId);
    } finally {
      await initialContext.close();
    }

    const reopenedContext = await openOwnerContext(browser, storageState);
    try {
      const reopenedPage = await reopenedContext.newPage();
      const exactPath = await openSavedCompositionFromHome(
        reopenedPage,
        seed.spaceId,
        compositionId,
        revisionId,
        manifest.composition_name,
      );
      await expect(reopenedPage).toHaveURL(
        new RegExp(`${exactPath.replaceAll("/", "\\/")}$`),
      );
      await expect(reopenedPage.getByLabel("month_start", { exact: true }))
        .toHaveValue(
          manifest.parameters.month_start,
        );
      await expect(reopenedPage.getByLabel("month_end", { exact: true }))
        .toHaveValue(
          manifest.parameters.month_end,
        );
      await expect(
        reopenedPage.getByLabel("month_start_other", { exact: true }),
      ).toHaveValue(
        manifest.parameters.month_start,
      );
      await expect(reopenedPage.getByLabel("month_start", { exact: true }))
        .toBeEditable();
      await expect(reopenedPage.getByLabel("month_end", { exact: true }))
        .toBeEditable();

      const independentQueryId = `${
        savedSqlVariablePrefix(seed.savedSqlName)
      }_table`;
      const changedQueryId = `${independentQueryId}_2`;
      const table = reopenedPage.getByRole("table", {
        name: `${independentQueryId} results`,
        exact: true,
      });
      const tableRegion = reopenedPage.locator(".compositionGraphValue")
        .filter({ has: table });
      await expect(table.locator("tbody tr")).toHaveCount(
        manifest.expected.first_page_rows,
      );
      await expect(table).toContainText(seed.rows[0]!.merchant);
      await expect(table).toContainText(
        seed.rows[manifest.expected.first_page_rows - 1]!.merchant,
      );
      await expect(tableRegion.getByRole("button", {
        name: "Next",
        exact: true,
      })).toBeEnabled();
      const secondTable = reopenedPage.getByRole("table", {
        name: `${changedQueryId} results`,
        exact: true,
      });
      const secondTableRegion = reopenedPage.locator(".compositionGraphValue")
        .filter({ has: secondTable });
      await expect(secondTable.locator("tbody tr")).toHaveCount(
        manifest.expected.first_page_rows,
      );
      await expect(secondTableRegion.getByRole("button", {
        name: "Next",
        exact: true,
      })).toBeEnabled();

      const evaluations: Array<Record<string, unknown>> = [];
      reopenedPage.on("request", (outgoing) => {
        const url = new URL(outgoing.url());
        if (
          outgoing.method() === "POST" &&
          url.pathname.endsWith(`/spaces/${seed.spaceId}/compositions/evaluate`)
        ) {
          try {
            evaluations.push(
              outgoing.postDataJSON() as Record<string, unknown>,
            );
          } catch {
            evaluations.push({});
          }
        }
      });

      await tableRegion.getByRole("button", { name: "Next", exact: true })
        .click();
      await expect(table.locator("tbody tr")).toHaveCount(
        manifest.expected.second_page_rows,
      );
      const secondPageRow = seed.rows.find(({ page }) => page === 2);
      expect(secondPageRow).toBeTruthy();
      await expect(table).toContainText(secondPageRow!.merchant);

      const evaluationCountBeforeChange = evaluations.length;
      await reopenedPage.getByLabel("month_start_other", { exact: true }).fill(
        manifest.parameters.month_start_after_change,
      );
      await expect(secondTable.locator("tbody tr")).toHaveCount(
        manifest.expected.after_month_start_change_rows,
      );
      await expect.poll(() => evaluations.length).toBe(
        evaluationCountBeforeChange + 1,
      );
      expect(evaluations.at(-1)?.requested_queries).toEqual([changedQueryId]);
      expect(evaluations.at(-1)?.requested_queries).not.toContain(
        independentQueryId,
      );
      // The changed Table is empty, while the unrelated Table stays on page 2.
      await expect(table.locator("tbody tr")).toHaveCount(
        manifest.expected.second_page_rows,
      );
      await expect(table).toContainText(secondPageRow!.merchant);
    } finally {
      await reopenedContext.close();
    }
  });
});
