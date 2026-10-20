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
import {
  completeCompositionGraphYaml,
  type CompositionGraphSeed,
} from "./lib/composition-graph.ts";

type SeedState = {
  spaceId: string;
  compositionId: string;
  revisionId: string;
};

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

async function createSpace(request: APIRequestContext): Promise<string> {
  return await getDefaultSpaceId(request);
}

async function seedInvalidInputComposition(
  request: APIRequestContext,
): Promise<SeedState> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `InvalidInput${suffix}`;
  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Amount\n`,
        fields: { Amount: { type: "integer", required: true } },
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
  const relation = form?.sql_relation;
  const amountColumn = form?.fields?.Amount?.sql_column;
  expect(relation).toBeTruthy();
  expect(amountColumn).toBeTruthy();

  for (const amount of [2, 5, 7]) {
    const entryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      { data: { form: formName, fields: { Amount: amount } } },
    );
    expect(entryResponse.status()).toBe(201);
  }

  const sqlResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name: `Minimum amount count ${suffix}`,
        kind: "user-query",
        sql: `SELECT COUNT(*) AS matching_rows FROM ${
          sqlIdentifier(relation!)
        } WHERE ${sqlIdentifier(amountColumn!)} >= $minimum`,
        variables: [{
          type: "integer",
          name: "minimum",
          description: "Minimum amount to include.",
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

  const graph: CompositionGraphSeed = {
    name: `Invalid variable input ${suffix}`,
    variables: [
      {
        id: "minimum_amount",
        kind: "input",
        value_type: { scalar_type: "integer", nullable: false },
        required: true,
        default: { kind: "integer", value: 5 },
      },
      {
        id: "matching_rows",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: {
          kind: "saved_sql",
          entry_id: savedSql.id!,
          revision_id: savedSql.revision_id!,
          arguments: [{ name: "minimum", variable: "minimum_amount" }],
          form_bindings: [],
          schema_fingerprint: "",
        },
      },
    ],
    components: [
      { id: "first_input", kind: "input", variable: "minimum_amount" },
      { id: "second_input", kind: "input", variable: "minimum_amount" },
      { id: "matching_rows_metric", kind: "metric", variable: "matching_rows" },
    ],
    rows: [
      {
        id: "inputs",
        items: [
          { kind: "component", component: "first_input" },
          { kind: "component", component: "second_input" },
        ],
      },
      {
        id: "result",
        items: [{ kind: "component", component: "matching_rows_metric" }],
      },
    ],
  };
  const yaml = await completeCompositionGraphYaml(request, spaceId, graph);
  const compositionResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions`),
    {
      headers: { "Idempotency-Key": crypto.randomUUID() },
      data: { yaml },
    },
  );
  expect([200, 201]).toContain(compositionResponse.status());
  const composition = await compositionResponse.json() as {
    composition_id?: string;
    revision_id?: string;
  };
  expect(composition.composition_id).toBeTruthy();
  expect(composition.revision_id).toBeTruthy();
  return {
    spaceId,
    compositionId: composition.composition_id!,
    revisionId: composition.revision_id!,
  };
}

async function settleWithoutMetricQuery(
  page: Page,
  getMetricQueryCount: () => number,
  expectedCount: number,
): Promise<void> {
  // Composition resolution and metric execution each have a debounce window.
  await page.waitForTimeout(400);
  expect(getMetricQueryCount()).toBe(expectedCount);
}

test.describe("Composition invalid variable Input", () => {
  let seed: SeedState;

  test.beforeAll(async ({ request }) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    seed = await seedInvalidInputComposition(request);
  });

  test("invalid_raw_input_blocks_query_and_stale_result", async ({ page }) => {
    test.setTimeout(120_000);
    await page.setViewportSize({ width: 390, height: 844 });
    let evaluationCount = 0;
    const evaluationPath = `/api/spaces/${seed.spaceId}/compositions/evaluate`;
    page.on("request", (request) => {
      if (
        request.method() === "POST" &&
        new URL(request.url()).pathname === evaluationPath
      ) evaluationCount += 1;
    });

    await page.goto(
      getFrontendUrl(
        `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/${seed.revisionId}`,
      ),
      { waitUntil: "domcontentloaded" },
    );

    const inputs = page.locator(".compositionGraphInput input");
    const firstInput = inputs.nth(0);
    const secondInput = inputs.nth(1);
    const metric = page.locator(".compositionGraphMetric strong");
    await expect(firstInput).toHaveAccessibleName("minimum_amount");
    await expect(secondInput).toHaveAccessibleName("minimum_amount");
    const firstInputBox = await firstInput.boundingBox();
    const secondInputBox = await secondInput.boundingBox();
    expect(firstInputBox).not.toBeNull();
    expect(secondInputBox).not.toBeNull();
    expect(firstInputBox!.height).toBeGreaterThanOrEqual(44);
    expect(secondInputBox!.height).toBeGreaterThanOrEqual(44);
    expect(
      await page.evaluate(() =>
        document.documentElement.scrollWidth <=
          document.documentElement.clientWidth
      ),
    ).toBe(true);
    await firstInput.focus();
    await expect(firstInput).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(secondInput).toBeFocused();
    await expect(firstInput).toHaveValue("5");
    await expect(secondInput).toHaveValue("5");
    await expect(metric).toHaveText("2", { timeout: 30_000 });
    expect(evaluationCount).toBeGreaterThan(0);
    const successfulEvaluationCount = evaluationCount;

    await firstInput.fill("-");
    await expect(firstInput).toHaveValue("-");
    await expect(secondInput).toHaveValue("-");
    await expect(firstInput).toHaveAttribute("aria-invalid", "true");
    await expect(secondInput).toHaveAttribute("aria-invalid", "true");
    await expect(metric).toHaveCount(0);
    await expect(page.getByRole("alert").first()).toBeVisible();
    await settleWithoutMetricQuery(
      page,
      () => evaluationCount,
      successfulEvaluationCount,
    );

    const explicitUnsetEvaluation = page.waitForResponse((response) => {
      const url = new URL(response.url());
      if (
        response.request().method() !== "POST" ||
        url.pathname !==
          `/api/spaces/${seed.spaceId}/compositions/evaluate`
      ) return false;
      try {
        const body = response.request().postDataJSON() as {
          unset_variables?: string[];
        };
        return body.unset_variables?.includes("minimum_amount") ?? false;
      } catch {
        return false;
      }
    });
    await secondInput.fill("");
    await expect(firstInput).toHaveValue("");
    await expect(secondInput).toHaveValue("");
    await expect(firstInput).toHaveAttribute("aria-invalid", "true");
    await expect(secondInput).toHaveAttribute("aria-invalid", "true");
    await expect(metric).toHaveCount(0);
    const unsetResponse = await explicitUnsetEvaluation;
    expect(unsetResponse.ok()).toBe(true);
    const unsetResult = await unsetResponse.json() as {
      ok?: boolean;
      diagnostics?: Array<{ code?: string; variable_id?: string }>;
    };
    expect(unsetResult.ok).toBe(false);
    expect(unsetResult.diagnostics).toContainEqual({
      code: "variable_missing",
      variable_id: "minimum_amount",
    });
    await settleWithoutMetricQuery(
      page,
      () => evaluationCount,
      successfulEvaluationCount + 1,
    );

    await firstInput.fill("2");
    await expect(firstInput).toHaveValue("2");
    await expect(secondInput).toHaveValue("2");
    await expect(metric).toHaveText("3", { timeout: 30_000 });
    expect(evaluationCount).toBeGreaterThan(successfulEvaluationCount);
  });
});
