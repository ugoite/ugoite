import {
  type APIRequestContext,
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
import { completeCompositionGraphYaml } from "./lib/composition-graph.ts";

const STALE_MERCHANT = "Stale-A";
const CURRENT_MERCHANT = "Current-B";
const CURRENT_AMOUNT = 8;

type SeedState = {
  spaceId: string;
  compositionId: string;
  revisionId: string;
};

type EvaluationRequest = {
  variables: Record<string, unknown>;
};

async function getFixtureSpace(request: APIRequestContext): Promise<string> {
  // Space creation requires a recent passkey assertion. This long hosted E2E
  // lane may reach this fixture after that assertion expires; UUID-suffixed
  // Forms and Compositions keep its records isolated within the default Space.
  return await getDefaultSpaceId(request);
}

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

async function seedStaleComposition(
  request: APIRequestContext,
): Promise<SeedState> {
  const spaceId = await getFixtureSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `StaleWindow${suffix}`;
  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Amount\n\n## Merchant\n`,
        fields: {
          Amount: { type: "double", required: true },
          Merchant: { type: "string", required: true },
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
    name?: string;
    sql_relation?: string;
    fields?: Record<string, { sql_column?: string }>;
  }>;
  const form = forms.find((candidate) => candidate.name === formName);
  const amountColumn = form?.fields?.Amount?.sql_column;
  const merchantColumn = form?.fields?.Merchant?.sql_column;
  expect(form?.sql_relation).toBeTruthy();
  expect(amountColumn).toBeTruthy();
  expect(merchantColumn).toBeTruthy();

  for (
    const row of [
      { Amount: 7, Merchant: STALE_MERCHANT },
      { Amount: CURRENT_AMOUNT, Merchant: CURRENT_MERCHANT },
    ]
  ) {
    const entryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      { data: { form: formName, fields: row } },
    );
    expect(entryResponse.status()).toBe(201);
  }

  const relation = sqlIdentifier(form!.sql_relation!);
  const amount = sqlIdentifier(amountColumn!);
  const merchant = sqlIdentifier(merchantColumn!);
  const yaml = await completeCompositionGraphYaml(request, spaceId, {
    name: `Stale graph ${suffix}`,
    variables: [
      {
        id: "minimum_amount",
        kind: "input",
        value_type: { scalar_type: "float", nullable: false },
        required: true,
        default: { kind: "float", value: 0 },
      },
      {
        id: "rows",
        kind: "query",
        returns: { kind: "table", columns: [] },
        query: {
          kind: "inline_sql",
          sql:
            `SELECT ${merchant} AS merchant, ${amount} AS amount FROM ${relation} WHERE ${amount} >= $minimum ORDER BY ${merchant} ASC`,
          arguments: [{ name: "minimum", variable: "minimum_amount" }],
        },
      },
      {
        id: "total",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "float", nullable: true },
        },
        query: {
          kind: "inline_sql",
          sql:
            `SELECT MAX(${amount}) AS total FROM ${relation} WHERE ${amount} >= $minimum`,
          arguments: [{ name: "minimum", variable: "minimum_amount" }],
        },
      },
    ],
    components: [
      { id: "amount_input", kind: "input", variable: "minimum_amount" },
      { id: "total_metric", kind: "metric", variable: "total" },
      { id: "result_rows", kind: "table", variable: "rows" },
    ],
    rows: [
      {
        id: "controls",
        items: [{ kind: "component", component: "amount_input" }],
      },
      {
        id: "results",
        items: [
          { kind: "component", component: "total_metric" },
          { kind: "component", component: "result_rows" },
        ],
      },
    ],
  });
  const saveResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions`),
    { headers: { "Idempotency-Key": crypto.randomUUID() }, data: { yaml } },
  );
  expect([200, 201]).toContain(saveResponse.status());
  const saved = await saveResponse.json() as {
    composition_id?: string;
    revision_id?: string;
  };
  expect(saved.composition_id).toBeTruthy();
  expect(saved.revision_id).toBeTruthy();
  return {
    spaceId,
    compositionId: saved.composition_id!,
    revisionId: saved.revision_id!,
  };
}

async function expectCurrentValues(page: Page): Promise<void> {
  await expect(page.locator(".compositionGraphMetric strong")).toHaveText(
    String(CURRENT_AMOUNT),
  );
  const table = page.getByRole("table");
  await expect(table).toContainText(CURRENT_MERCHANT);
  await expect(table).not.toContainText(STALE_MERCHANT);
  await expect(page.locator("[role='alert']")).toHaveCount(0);
  await expect(page.locator("[role='status']")).toHaveCount(0);
}

test.describe("Composition Stale Response", () => {
  let seed: SeedState;

  test.beforeAll(async ({ request }) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    seed = await seedStaleComposition(request);
  });

  test("a late evaluation for an earlier input cannot replace current graph values", async ({ page }) => {
    test.setTimeout(180_000);
    let release!: () => void;
    const heldGate = new Promise<void>((resolve) => release = resolve);
    let held = 0;
    let delivered = 0;
    await page.route("**/compositions/evaluate", async (route) => {
      const request = route.request().postDataJSON() as EvaluationRequest;
      const response = await route.fetch();
      if (request.variables.minimum_amount !== 7) {
        await route.fulfill({ response });
        return;
      }
      held += 1;
      await heldGate;
      delivered += 1;
      await route.fulfill({ response });
    });

    await page.goto(
      getFrontendUrl(
        `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/${seed.revisionId}`,
      ),
      { waitUntil: "domcontentloaded" },
    );
    const threshold = page.getByRole("textbox", { name: "minimum_amount" });
    await threshold.fill("7");
    await expect.poll(() => held).toBeGreaterThan(0);

    await threshold.fill(String(CURRENT_AMOUNT));
    await expectCurrentValues(page);
    const currentSnapshot = await page.locator(".compositionGraphRender")
      .textContent() ?? "";
    expect(currentSnapshot).toContain(CURRENT_MERCHANT);
    expect(currentSnapshot).not.toContain(STALE_MERCHANT);

    release();
    await expect.poll(() => delivered).toBe(held);
    await expectCurrentValues(page);
    await expect(page.locator(".compositionGraphRender")).toHaveText(
      currentSnapshot,
    );
  });
});
