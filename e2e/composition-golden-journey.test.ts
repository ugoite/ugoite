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
  id?: number;
  query_capability?: { field?: { field_id?: number } };
};

type FormSummary = {
  id?: string;
  name?: string;
  fields?: Record<string, FormField>;
};

type SeedState = {
  spaceId: string;
  compositionId: string;
  revisionId: string;
  merchantByDay: Record<string, string>;
};

type SeedManifest = {
  source_fixture: string;
  model_connection: "disabled";
  composition_name: string;
  page_limit: number;
  parameters: {
    month_start: string;
    month_end: string;
    month_start_after_change: string;
  };
  entries: Array<{
    date: string;
    amount: number;
    page: 1 | 2 | "excluded";
  }>;
  saved_sql: {
    sql: string;
    variables: Array<{ type: string; name: string; description: string }>;
  };
  expected: {
    first_page_rows: number;
    second_page_rows: number;
    after_month_start_change_rows: number;
    metric_scalar: number;
  };
};

async function readSeedManifest(): Promise<SeedManifest> {
  return JSON.parse(
    await Deno.readTextFile(SEED_MANIFEST_URL),
  ) as SeedManifest;
}

function replaceExactlyOnce(
  value: string,
  search: string,
  replacement: string,
): string {
  const first = value.indexOf(search);
  if (first < 0 || value.indexOf(search, first + search.length) >= 0) {
    throw new Error(`Expected one Composition fixture token: ${search}`);
  }
  return `${value.slice(0, first)}${replacement}${
    value.slice(first + search.length)
  }`;
}

function fieldId(form: FormSummary, name: string): number {
  const field = form.fields?.[name];
  const id = field?.query_capability?.field?.field_id ?? field?.id;
  if (!Number.isInteger(id) || (id ?? 0) < 1) {
    throw new Error(`Form field ${name} has no stable query field ID`);
  }
  return id!;
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

async function seedJourney(
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
        template: `# ${formName}\n\n## Date\n\n## Merchant\n\n## Amount\n`,
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
  const dateFieldId = fieldId(form!, "Date");
  const merchantFieldId = fieldId(form!, "Merchant");
  const amountFieldId = fieldId(form!, "Amount");

  const merchantByDay: Record<string, string> = {};
  for (const { date, amount } of manifest.entries) {
    const merchant = `Golden-${date}-${suffix}`;
    const entryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      {
        data: {
          form: formName,
          fields: { Date: date, Merchant: merchant, Amount: amount },
        },
      },
    );
    expect(entryResponse.status()).toBe(201);
    merchantByDay[date] = merchant;
  }

  const sqlResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name: `Golden metric ${suffix}`,
        kind: "user-query",
        sql: manifest.saved_sql.sql,
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

  const fixtureUrl = new URL(`../${manifest.source_fixture}`, import.meta.url);
  let yaml = await Deno.readTextFile(fixtureUrl);
  yaml = replaceExactlyOnce(
    yaml,
    'form_id: "00000000-0000-7000-8000-000000000010"',
    `form_id: "${form!.id}"`,
  );
  yaml = replaceExactlyOnce(
    yaml,
    'entry_id: "00000000-0000-7000-8000-000000000020"',
    `entry_id: "${savedSql.id}"`,
  );
  yaml = replaceExactlyOnce(
    yaml,
    'revision_id: "00000000-0000-7000-8000-000000000021"',
    `revision_id: "${savedSql.revision_id}"`,
  );
  yaml = replaceExactlyOnce(
    yaml,
    "      query:\n        filters:",
    `      query:\n        page_limit: ${manifest.page_limit}\n        filters:`,
  );
  const fieldIds = new Map([
    ["100", dateFieldId],
    ["101", merchantFieldId],
    ["102", amountFieldId],
  ]);
  yaml = yaml.replace(
    /field_id: (100|101|102)\b/g,
    (_match, sourceId: string) => `field_id: ${fieldIds.get(sourceId)}`,
  );
  yaml = replaceExactlyOnce(
    yaml,
    "fields: [100, 101, 102]",
    `fields: [${dateFieldId}, ${merchantFieldId}, ${amountFieldId}]`,
  );
  expect(yaml).toContain(`page_limit: ${manifest.page_limit}`);

  const saveResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions`),
    { data: { yaml } },
  );
  expect([200, 201]).toContain(saveResponse.status());
  const saved = await saveResponse.json() as {
    composition_id?: string;
    revision_id?: string;
    receipt?: { committed_revision_ids?: string[] };
  };
  expect(saved.composition_id).toBeTruthy();
  expect(saved.revision_id).toBeTruthy();
  expect(saved.receipt?.committed_revision_ids).toContain(saved.revision_id);

  return {
    spaceId,
    compositionId: saved.composition_id!,
    revisionId: saved.revision_id!,
    merchantByDay,
  };
}

async function openOwnerContext(
  browser: Browser,
  storageState: Awaited<ReturnType<BrowserContext["storageState"]>>,
) {
  return await browser.newContext({ storageState });
}

async function openSavedCompositionFromHome(
  page: Page,
  seed: SeedState,
  compositionName: string,
): Promise<string> {
  const path =
    `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/${seed.revisionId}`;
  await page.goto(getFrontendUrl(`/spaces/${seed.spaceId}/dashboard`), {
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
    await waitForServers(request);
    manifest = await readSeedManifest();
    expect(manifest.model_connection).toBe("disabled");
    expect(manifest.expected.first_page_rows).toBe(manifest.page_limit);
    seed = await seedJourney(request, manifest);
  });

  test("Browser reopens a saved Composition with model connection disabled", async ({ browser, context }) => {
    const storageState = await context.storageState();
    const initialContext = await openOwnerContext(browser, storageState);
    try {
      const initialPage = await initialContext.newPage();
      const route = await openSavedCompositionFromHome(
        initialPage,
        seed,
        manifest.composition_name,
      );
      await expect(initialPage.getByLabel("month_start")).toBeVisible();
      await initialPage.getByLabel("month_start").fill(
        manifest.parameters.month_start,
      );
      await initialPage.getByLabel("month_end").fill(
        manifest.parameters.month_end,
      );
      const initialTable = initialPage.getByRole("table");
      await expect(initialTable.locator("tbody tr")).toHaveCount(
        manifest.expected.first_page_rows,
      );
      await expect(initialPage.locator("output.compositionMetric"))
        .toHaveText(String(manifest.expected.metric_scalar));
      for (const entry of manifest.entries.filter(({ page }) => page === 1)) {
        await expect(initialTable).toContainText(
          seed.merchantByDay[entry.date],
        );
      }
      expect(initialPage.url()).toContain(route);
    } finally {
      await initialContext.close();
    }

    const reopenedContext = await openOwnerContext(browser, storageState);
    try {
      const reopenedPage = await reopenedContext.newPage();
      const route = await openSavedCompositionFromHome(
        reopenedPage,
        seed,
        manifest.composition_name,
      );
      await expect(reopenedPage.getByLabel("month_start")).toHaveValue("");
      await expect(reopenedPage).toHaveURL(
        new RegExp(`${route.replaceAll("/", "\\/")}$`),
      );
    } finally {
      await reopenedContext.close();
    }
  });

  test("Composition remains portable across Space reopen and query pagination", async ({ browser, context }) => {
    const reopenedContext = await openOwnerContext(
      browser,
      await context.storageState(),
    );
    try {
      const page = await reopenedContext.newPage();
      await openSavedCompositionFromHome(page, seed, manifest.composition_name);
      await page.getByLabel("month_start").fill(
        manifest.parameters.month_start,
      );
      await page.getByLabel("month_end").fill(manifest.parameters.month_end);

      const table = page.getByRole("table");
      await expect(table.locator("tbody tr")).toHaveCount(
        manifest.expected.first_page_rows,
      );
      await expect(page.locator("output.compositionMetric")).toHaveText(
        String(manifest.expected.metric_scalar),
      );
      for (const entry of manifest.entries.filter(({ page }) => page === 1)) {
        await expect(table).toContainText(seed.merchantByDay[entry.date]);
      }

      await page.getByRole("button", { name: "Next" }).click();
      await expect(table.locator("tbody tr")).toHaveCount(
        manifest.expected.second_page_rows,
      );
      for (const entry of manifest.entries.filter(({ page }) => page === 2)) {
        await expect(table).toContainText(seed.merchantByDay[entry.date]);
      }
      await expect(page.locator("output.compositionMetric")).toHaveText(
        String(manifest.expected.metric_scalar),
      );

      await page.getByLabel("month_start").fill(
        manifest.parameters.month_start_after_change,
      );
      await expect(table.locator("tbody tr")).toHaveCount(
        manifest.expected.after_month_start_change_rows,
      );
      const filteredEntries = manifest.entries.filter(({ date }) =>
        date >= manifest.parameters.month_start_after_change &&
        date < manifest.parameters.month_end
      );
      for (const entry of filteredEntries) {
        await expect(table).toContainText(seed.merchantByDay[entry.date]);
      }
      for (
        const entry of manifest.entries.filter(({ date }) =>
          date < manifest.parameters.month_start_after_change
        )
      ) {
        await expect(table).not.toContainText(seed.merchantByDay[entry.date]);
      }
      await expect(page.locator("output.compositionMetric")).toHaveText(
        String(manifest.expected.metric_scalar),
      );
      await expect(page.getByRole("button", { name: "Next" })).toBeDisabled();
    } finally {
      await reopenedContext.close();
    }
  });
});
