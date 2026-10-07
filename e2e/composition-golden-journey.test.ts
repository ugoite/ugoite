import {
  type APIRequestContext,
  type Browser,
  type BrowserContext,
  expect,
  type Page,
  test,
} from "@playwright/test";
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
    await Deno.readTextFile(SEED_MANIFEST_URL),
  ) as SeedManifest;
}

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
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
  for (const row of rows) {
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
    expect(entryResponse.status()).toBe(201);
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
  const link = page.getByRole("link", { name: new RegExp(compositionName) });
  await expect(link).toHaveAttribute("href", path);
  await link.click();
  await expect(page).toHaveURL(new RegExp(`${path.replaceAll("/", "\\/")}$`));
  await expect(
    page.getByRole("heading", { level: 1, name: compositionName }),
  ).toBeVisible();
  return path;
}

test.describe("Composition Golden Journey", () => {
  let seed: SeedState;
  let manifest: SeedManifest;

  test.beforeAll(async ({ request }) => {
    test.setTimeout(120_000);
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
          `/spaces/${seed.spaceId}/compositions/new$`,
        ),
      );
      // The Studio seed prefills the tool name from the Saved SQL entry and
      // carries its exact revision as the first source with a default Table,
      // so the canvas owns a visible block and no picker interaction is
      // needed before saving.
      const nameInput = initialPage.getByLabel("Name", { exact: true });
      await expect(nameInput).toHaveValue(seed.savedSqlName);
      await expect(
        initialPage.getByRole("button", {
          name: `Select ${seed.savedSqlName}`,
          exact: true,
        }),
      ).toBeVisible();
      await nameInput.fill(manifest.composition_name);
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
      await expect(reopenedPage.getByLabel("month_start")).toHaveValue(
        manifest.parameters.month_start,
      );
      await expect(reopenedPage.getByLabel("month_end")).toHaveValue(
        manifest.parameters.month_end,
      );

      const table = reopenedPage.getByRole("table");
      await expect(table.locator("tbody tr")).toHaveCount(
        manifest.expected.first_page_rows,
      );
      await expect(table).toContainText(seed.rows[0]!.merchant);
      await expect(table).toContainText(
        seed.rows[manifest.expected.first_page_rows - 1]!.merchant,
      );
      await expect(
        reopenedPage.getByRole("button", { name: "Next", exact: true }),
      ).toBeEnabled();

      await reopenedPage.getByRole("button", { name: "Next", exact: true })
        .click();
      await expect(table.locator("tbody tr")).toHaveCount(
        manifest.expected.second_page_rows,
      );
      const secondPageRow = seed.rows.find(({ page }) => page === 2);
      expect(secondPageRow).toBeTruthy();
      await expect(table).toContainText(secondPageRow!.merchant);

      await reopenedPage.getByLabel("month_start").fill(
        manifest.parameters.month_start_after_change,
      );
      await expect(table.locator("tbody tr")).toHaveCount(
        manifest.expected.after_month_start_change_rows,
      );
      await expect(reopenedPage.getByText("No results", { exact: true }))
        .toBeVisible();
      // ResultPagination intentionally renders no navigation on a single
      // (here empty) page, so accept absent-or-disabled pagination.
      await expect(
        reopenedPage.getByRole("button", { name: "Next", exact: true }),
      ).toHaveCount(0);
      await expect(
        reopenedPage.getByRole("button", { name: "Previous", exact: true }),
      ).toHaveCount(0);
    } finally {
      await reopenedContext.close();
    }
  });
});
