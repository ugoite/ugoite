import { type APIRequestContext, expect, test } from "@playwright/test";
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

type SavedSqlRevision = { entryId: string; revisionId: string };

type SeedState = {
  spaceId: string;
  formName: string;
  compositionId: string;
  revisionId: string;
  compositionName: string;
};

type PersistedCompositionWorkSnapshot = {
  compositionRevisionIds: string[];
  compositionRevisionCount: number;
  forms: Array<{ id?: string; name?: string }>;
  entryCount: number;
};

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

async function createSpace(request: APIRequestContext): Promise<string> {
  return await getDefaultSpaceId(request);
}

async function createSavedSql(
  request: APIRequestContext,
  spaceId: string,
  name: string,
  sql: string,
): Promise<SavedSqlRevision> {
  const response = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name,
        kind: "user-query",
        sql,
        variables: [{
          type: "float",
          name: "minimum",
          description: "Minimum amount to include.",
        }],
      },
    },
  );
  expect([200, 201]).toContain(response.status());
  const savedSql = await response.json() as {
    id?: string;
    revision_id?: string;
  };
  expect(savedSql.id).toBeTruthy();
  expect(savedSql.revision_id).toBeTruthy();
  return { entryId: savedSql.id!, revisionId: savedSql.revision_id! };
}

async function seedComposition(
  request: APIRequestContext,
): Promise<SeedState> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `PageMetric${suffix}`;
  const compositionName = `Page metric ${suffix}`;

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
    id?: string;
    name?: string;
    sql_relation?: string;
    fields?: Record<string, {
      sql_column?: string;
    }>;
  }>;
  const form = forms.find((candidate) => candidate.name === formName);
  const relation = form?.sql_relation;
  const amountColumn = form?.fields?.Amount?.sql_column;
  expect(relation).toBeTruthy();
  expect(form?.id).toBeTruthy();
  expect(amountColumn).toBeTruthy();
  const merchantColumn = form?.fields?.Merchant?.sql_column;
  expect(merchantColumn).toBeTruthy();

  // Composition SQL uses the shared 100-row page size. Keep one row beyond
  // the first page so Table pagination remains observable.
  const amounts = Array.from({ length: 102 }, (_, amount) => amount);
  for (let offset = 0; offset < amounts.length; offset += 8) {
    await Promise.all(
      amounts.slice(offset, offset + 8).map(async (amount) => {
        const entryResponse = await request.post(
          getBackendUrl(`/spaces/${spaceId}/entries`),
          {
            data: {
              form: formName,
              fields: {
                Amount: amount,
                Merchant: `Pager-${String(amount).padStart(3, "0")}`,
              },
            },
          },
        );
        expect(entryResponse.status()).toBe(201);
      }),
    );
  }

  const metricSql = await createSavedSql(
    request,
    spaceId,
    `Matching count ${suffix}`,
    `SELECT COUNT(*) AS matching_count FROM ${sqlIdentifier(relation!)} WHERE ${
      sqlIdentifier(amountColumn!)
    } >= $minimum`,
  );
  const tableSql = await createSavedSql(
    request,
    spaceId,
    `Paged table source ${suffix}`,
    `SELECT ${sqlIdentifier(amountColumn!)} AS amount, ${
      sqlIdentifier(merchantColumn!)
    } AS merchant FROM ${sqlIdentifier(relation!)} WHERE ${
      sqlIdentifier(amountColumn!)
    } >= $minimum ORDER BY ${sqlIdentifier(amountColumn!)} ASC`,
  );

  const graph: CompositionGraphSeed = {
    name: compositionName,
    variables: [
      {
        id: "minimum_amount",
        kind: "input",
        value_type: { scalar_type: "float", nullable: false },
        required: true,
        default: { kind: "float", value: 1 },
      },
      {
        id: "paged_rows",
        kind: "query",
        returns: { kind: "table", columns: [] },
        query: {
          kind: "saved_sql",
          entry_id: tableSql.entryId,
          revision_id: tableSql.revisionId,
          label_source: "variable",
          arguments: [{ name: "minimum", variable: "minimum_amount" }],
          form_bindings: [],
          schema_fingerprint: "",
        },
      },
      {
        id: "matching_count_value",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: {
          kind: "saved_sql",
          entry_id: metricSql.entryId,
          revision_id: metricSql.revisionId,
          label_source: "variable",
          column: "matching_count",
          arguments: [{ name: "minimum", variable: "minimum_amount" }],
          form_bindings: [],
          schema_fingerprint: "",
        },
      },
    ],
    components: [
      { id: "minimum_input", kind: "input", variable: "minimum_amount" },
      {
        id: "matching_count",
        kind: "metric",
        variable: "matching_count_value",
      },
      { id: "rows", kind: "table", variable: "paged_rows" },
    ],
    rows: [
      {
        id: "controls",
        items: [{ kind: "component", component: "minimum_input" }],
      },
      {
        id: "results",
        items: [
          { kind: "component", component: "matching_count" },
          { kind: "component", component: "rows" },
        ],
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
  const savedComposition = await compositionResponse.json() as {
    composition_id?: string;
    revision_id?: string;
  };
  expect(savedComposition.composition_id).toBeTruthy();
  expect(savedComposition.revision_id).toBeTruthy();

  return {
    spaceId,
    formName,
    compositionId: savedComposition.composition_id!,
    revisionId: savedComposition.revision_id!,
    compositionName,
  };
}

async function getSavedSqlSource(
  request: APIRequestContext,
  spaceId: string,
  savedSqlId: string,
): Promise<string> {
  const response = await request.get(
    getBackendUrl(`/spaces/${spaceId}/sql/${savedSqlId}`),
  );
  expect(response.ok()).toBe(true);
  const savedSql = await response.json() as { sql?: string };
  expect(savedSql.sql).toBeTruthy();
  return savedSql.sql!;
}

async function snapshotPersistedCompositionWork(
  request: APIRequestContext,
  seed: SeedState,
): Promise<PersistedCompositionWorkSnapshot> {
  const historyResponse = await request.get(
    getBackendUrl(
      `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/history?limit=100&offset=0`,
    ),
  );
  expect(historyResponse.ok()).toBe(true);
  const history = await historyResponse.json() as {
    revisions: Array<{ revision: { revision_id: string } }>;
    total: number;
  };
  expect(history.revisions).toHaveLength(history.total);

  const formsResponse = await request.get(
    getBackendUrl(`/spaces/${seed.spaceId}/forms`),
  );
  expect(formsResponse.ok()).toBe(true);
  const forms = await formsResponse.json() as Array<{
    id?: string;
    name?: string;
  }>;
  expect(forms.every((form) => form.id && form.name)).toBe(true);
  const sourceForms = forms.filter((form) => form.name === seed.formName);
  expect(sourceForms).toHaveLength(1);

  const entryCountResponse = await request.post(
    getBackendUrl(`/spaces/${seed.spaceId}/entries/query/count`),
    {
      data: {
        query: { scope: { kind: "all" }, filters: [], sort: [] },
      },
    },
  );
  expect(entryCountResponse.ok()).toBe(true);
  const entryCount = await entryCountResponse.json() as { count: number };
  expect(Number.isInteger(entryCount.count)).toBe(true);

  return {
    compositionRevisionIds: history.revisions.map(({ revision }) =>
      revision.revision_id
    ).sort(),
    compositionRevisionCount: history.total,
    forms: forms.map(({ id, name }) => ({ id, name })).sort((a, b) =>
      (a.id ?? "").localeCompare(b.id ?? "")
    ),
    entryCount: entryCount.count,
  };
}

test.describe("Composition Table and Metric pagination", () => {
  let seed: SeedState;

  test.beforeAll(async ({ request }) => {
    // The fixture publishes 102 Entry revisions over the direct API before
    // it can exercise the 100-row SQL page boundary. Hosted filesystem
    // latency can make this seed exceed seven minutes. CI evidence showed
    // twelve parallel batches had started but two writes were still pending
    // at the seven-minute bound; allow the final batches to complete within
    // the job's 45-minute limit.
    test.setTimeout(720_000);
    await waitForServers(request);
    seed = await seedComposition(request);
  });

  test("Table pagination leaves the SQL Metric and typed Input unchanged", async ({ page, request }) => {
    test.setTimeout(120_000);
    // This scenario resolves a Form-backed table and Saved SQL metric over
    // 102 Entries. Keep those async page assertions bounded without using the
    // five-second default for this larger fixture.
    const uiExpect = expect.configure({ timeout: 20_000 });
    let fullEvaluationCount = 0;
    page.on("request", (request) => {
      const url = new URL(request.url());
      if (
        request.method() === "POST" &&
        url.pathname.endsWith(`/spaces/${seed.spaceId}/compositions/evaluate`)
      ) {
        const body = request.postDataJSON() as { page?: unknown };
        if (!body.page) fullEvaluationCount += 1;
      }
    });

    const path =
      `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/${seed.revisionId}`;
    await page.goto(getFrontendUrl(`/spaces/${seed.spaceId}/compositions`), {
      waitUntil: "domcontentloaded",
    });
    const link = page.locator(`a[href="${path}"]`);
    await uiExpect(link).toBeVisible();
    await link.click();
    await uiExpect(page).toHaveURL(new RegExp(`${path}$`));

    const input = page.getByLabel("minimum_amount", { exact: true });
    const metric = page.locator(".compositionGraphMetric strong");
    const table = page.getByRole("table");
    await uiExpect(input).toHaveValue("1");
    await uiExpect(metric).toHaveText("101");
    await uiExpect(table.locator("tbody tr")).toHaveCount(100);
    await uiExpect(table).toContainText("Pager-001");
    await uiExpect(page.getByRole("button", { name: "Next", exact: true }))
      .toBeEnabled();
    const initialEvaluations = fullEvaluationCount;
    expect(initialEvaluations).toBeGreaterThan(0);

    await input.fill("0");
    await uiExpect(metric).toHaveText("102");
    await uiExpect(table.locator("tbody tr")).toHaveCount(100);
    await uiExpect(table).toContainText("Pager-000");
    const evaluationsAfterInputChange = fullEvaluationCount;
    expect(evaluationsAfterInputChange).toBeGreaterThan(
      initialEvaluations,
    );

    await page.getByRole("button", { name: "Next", exact: true }).click();
    await uiExpect(table.locator("tbody tr")).toHaveCount(2);
    await uiExpect(table).toContainText("Pager-100");
    await uiExpect(input).toHaveValue("0");
    await uiExpect(metric).toHaveText("102");
    expect(fullEvaluationCount).toBe(evaluationsAfterInputChange);

    await page.getByRole("button", { name: "Previous", exact: true }).click();
    await uiExpect(table.locator("tbody tr")).toHaveCount(100);
    await uiExpect(table).toContainText("Pager-000");
    await uiExpect(input).toHaveValue("0");
    await uiExpect(metric).toHaveText("102");
    expect(fullEvaluationCount).toBe(evaluationsAfterInputChange);

    // Reopen the same revision in Design. The graph preview uses its own
    // authorized evaluation path, but the table still receives only one
    // bounded page. Keep the server exchanges so this proves the browser did
    // not fetch and slice the remaining rows itself.
    const persistedBeforeDesign = await snapshotPersistedCompositionWork(
      request,
      seed,
    );
    type EvaluationRequest = {
      variables?: Record<string, unknown>;
      page?: { variable_id?: string; cursor?: string };
      requested_queries?: string[];
    };
    type EvaluationResponse = {
      ok: boolean;
      computed_query_ids: string[];
      query_results: Record<string, {
        value: { kind: string; rows?: unknown[][] };
        has_more: boolean;
        next?: string;
      }>;
    };
    const designPageRequests: EvaluationRequest[] = [];
    page.on("request", (outgoing) => {
      if (
        outgoing.method() !== "POST" ||
        !new URL(outgoing.url()).pathname.endsWith(
          `/spaces/${seed.spaceId}/compositions/evaluate`,
        )
      ) return;
      const body = outgoing.postDataJSON() as EvaluationRequest;
      if (body.page) designPageRequests.push(body);
    });

    const designInitialResponse = page.waitForResponse((incoming) =>
      incoming.request().method() === "POST" &&
      new URL(incoming.url()).pathname.endsWith(
        `/spaces/${seed.spaceId}/compositions/evaluate`,
      )
    );
    await page.goto(getFrontendUrl(`${path}/edit`), {
      waitUntil: "domcontentloaded",
    });
    const initialDesignEvaluation = await (await designInitialResponse)
      .json() as EvaluationResponse;
    expect(initialDesignEvaluation.ok).toBe(true);
    const initialDesignTable =
      initialDesignEvaluation.query_results.paged_rows;
    expect(initialDesignTable.value.kind).toBe("table");
    expect(initialDesignTable.value.rows).toHaveLength(100);
    expect(initialDesignTable.has_more).toBe(true);
    expect(initialDesignTable.next).toBeTruthy();
    expect(designPageRequests).toHaveLength(0);

    const designInput = page.getByRole("textbox", {
      name: "minimum_amount",
      exact: true,
    });
    const designMetric = page.locator(".compositionGraphMetric strong");
    const designTable = page.getByRole("table");
    await uiExpect(designInput).toHaveValue("1");
    await uiExpect(designMetric).toHaveText("101");
    await uiExpect(designTable.locator("tbody tr")).toHaveCount(100);

    const zeroInputEvaluation = page.waitForResponse((incoming) => {
      if (
        incoming.request().method() !== "POST" ||
        !new URL(incoming.url()).pathname.endsWith(
          `/spaces/${seed.spaceId}/compositions/evaluate`,
        )
      ) return false;
      const body = incoming.request().postDataJSON() as EvaluationRequest;
      return !body.page && body.variables?.minimum_amount === 0;
    });
    await designInput.fill("0");
    const zeroDesignEvaluation = await (await zeroInputEvaluation)
      .json() as EvaluationResponse;
    expect(zeroDesignEvaluation.ok).toBe(true);
    const zeroDesignTable = zeroDesignEvaluation.query_results.paged_rows;
    expect(zeroDesignTable.value.rows).toHaveLength(100);
    expect(zeroDesignTable.has_more).toBe(true);
    await uiExpect(designInput).toHaveValue("0");
    await uiExpect(designMetric).toHaveText("102");
    await uiExpect(designTable.locator("tbody tr")).toHaveCount(100);
    expect(designPageRequests).toHaveLength(0);

    const nextDesignPageResponse = page.waitForResponse((incoming) => {
      if (
        incoming.request().method() !== "POST" ||
        !new URL(incoming.url()).pathname.endsWith(
          `/spaces/${seed.spaceId}/compositions/evaluate`,
        )
      ) return false;
      const body = incoming.request().postDataJSON() as EvaluationRequest;
      return body.page?.variable_id === "paged_rows";
    });
    await page.getByRole("button", { name: "Next", exact: true }).click();
    const nextDesignEvaluation = await (await nextDesignPageResponse)
      .json() as EvaluationResponse;
    const requestedPage = designPageRequests.at(-1);
    expect(designPageRequests).toHaveLength(1);
    expect(requestedPage).toMatchObject({
      requested_queries: ["paged_rows"],
      page: {
        variable_id: "paged_rows",
        cursor: zeroDesignTable.next,
      },
    });
    expect(nextDesignEvaluation.ok).toBe(true);
    expect(nextDesignEvaluation.computed_query_ids).toEqual(["paged_rows"]);
    const nextDesignTable = nextDesignEvaluation.query_results.paged_rows;
    expect(nextDesignTable.value.kind).toBe("table");
    expect(nextDesignTable.value.rows).toHaveLength(2);
    expect(JSON.stringify(nextDesignTable.value.rows)).toContain("Pager-100");
    expect(JSON.stringify(nextDesignTable.value.rows)).toContain("Pager-101");
    expect(nextDesignTable.has_more).toBe(false);
    await uiExpect(designTable.locator("tbody tr")).toHaveCount(2);
    await uiExpect(designTable).toContainText("Pager-100");
    await uiExpect(designTable).toContainText("Pager-101");
    await uiExpect(designInput).toHaveValue("0");
    await uiExpect(designMetric).toHaveText("102");

    await page.getByRole("button", { name: "Previous", exact: true }).click();
    await uiExpect(designTable.locator("tbody tr")).toHaveCount(100);
    await uiExpect(designTable).toContainText("Pager-000");
    await uiExpect(designInput).toHaveValue("0");
    await uiExpect(designMetric).toHaveText("102");
    expect(designPageRequests).toHaveLength(1);

    expect(await snapshotPersistedCompositionWork(request, seed)).toEqual(
      persistedBeforeDesign,
    );
  });

  test("Studio counts the pinned Table SQL separately and preserves its source while paging", async ({ page, request }) => {
    test.setTimeout(120_000);
    const uiExpect = expect.configure({ timeout: 20_000 });
    const revisionPath =
      `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/${seed.revisionId}`;
    const sourceBefore = await getSavedSqlSource(
      request,
      seed.spaceId,
      seed.tableSql.entryId,
    );
    expect(sourceBefore).not.toMatch(/\bLIMIT\b/i);

    await page.goto(getFrontendUrl(`${revisionPath}/edit`), {
      waitUntil: "domcontentloaded",
    });
    await page.getByRole("tab", { name: "Variables", exact: true }).click();
    await page.getByRole("button", { name: "minimum_amount", exact: true })
      .click();
    const inputDefault = page.getByLabel("Default", { exact: true });
    await inputDefault.fill("0");
    await uiExpect(inputDefault).toHaveValue("0");
    await page.getByRole("button", { name: "paged_rows", exact: true }).click();
    const studioRun = page.getByRole("button", {
      name: "Run Query",
      exact: true,
    });
    const evaluationRequests: Array<Record<string, unknown>> = [];
    const countRequests: Array<Record<string, unknown>> = [];
    page.on("request", (outgoing) => {
      const path = new URL(outgoing.url()).pathname;
      if (
        path.endsWith("/compositions/evaluate") &&
        outgoing.method() === "POST"
      ) {
        evaluationRequests.push(
          outgoing.postDataJSON() as Record<string, unknown>,
        );
      }
      if (path.endsWith("/sql/query/count") && outgoing.method() === "POST") {
        countRequests.push(outgoing.postDataJSON() as Record<string, unknown>);
      }
    });
    const firstPageResponse = page.waitForResponse((response) =>
      new URL(response.url()).pathname.endsWith("/compositions/evaluate")
    );
    await studioRun.click();
    const firstEvaluation = await (await firstPageResponse).json() as {
      query_results?: Record<string, {
        value?: { kind: string; rows?: unknown[][] };
        count_snapshot?: string;
        has_more: boolean;
        next?: string;
      }>;
    };
    const firstPage = firstEvaluation.query_results?.paged_rows;
    expect(firstPage?.value?.kind).toBe("table");
    expect(firstPage?.value?.rows).toHaveLength(100);
    expect(firstPage?.has_more).toBe(true);
    expect(firstPage?.next).toBeTruthy();
    expect(firstPage?.count_snapshot).toMatch(/^v1\./);
    const studioTable = page.getByRole("table");
    await uiExpect(studioTable.locator("tbody tr")).toHaveCount(100);
    await uiExpect(
      page.getByRole("button", { name: "Count rows", exact: true }),
    )
      .toBeEnabled();
    expect(countRequests).toHaveLength(0);
    expect(evaluationRequests.at(-1)).toMatchObject({
      requested_queries: ["paged_rows"],
    });

    const nextPageResponse = page.waitForResponse((response) =>
      new URL(response.url()).pathname.endsWith("/compositions/evaluate")
    );
    await page.getByRole("button", { name: "Next", exact: true }).click();
    await uiExpect(studioTable.locator("tbody tr")).toHaveCount(2);
    const nextEvaluation = await (await nextPageResponse).json() as {
      query_results?: Record<string, {
        value?: { kind: string; rows?: unknown[][] };
        has_more: boolean;
        count_snapshot?: string;
      }>;
    };
    const nextPage = nextEvaluation.query_results?.paged_rows;
    expect(nextPage?.value?.kind).toBe("table");
    expect(nextPage?.value?.rows).toHaveLength(2);
    expect(JSON.stringify(nextPage?.value?.rows)).toContain("Pager-100");
    expect(JSON.stringify(nextPage?.value?.rows)).toContain("Pager-101");
    expect(JSON.stringify(nextPage?.value?.rows)).not.toContain("Pager-102");
    expect(nextPage?.has_more).toBe(false);
    expect(evaluationRequests.at(-1)).toMatchObject({
      page: { variable_id: "paged_rows", cursor: firstPage?.next },
    });

    const appendedEntryResponse = await request.post(
      getBackendUrl(`/spaces/${seed.spaceId}/entries`),
      {
        data: {
          form: seed.formName,
          fields: { Amount: 102, Merchant: "Pager-102" },
        },
      },
    );
    expect(appendedEntryResponse.status()).toBe(201);

    const countResponse = page.waitForResponse((response) =>
      new URL(response.url()).pathname.endsWith("/sql/query/count")
    );
    await page.getByRole("button", { name: "Count rows", exact: true }).click();
    expect(await (await countResponse).json()).toMatchObject({ count: 102 });
    expect(countRequests.at(-1)).toMatchObject({
      sql: "",
      saved_sql: {
        id: seed.tableSql.entryId,
        revision_id: seed.tableSql.revisionId,
      },
      parameters: { minimum: 0 },
      composition_count_snapshot: nextPage?.count_snapshot,
    });
    expect(nextPage?.count_snapshot).toMatch(/^v1\./);
    expect(String(countRequests.at(-1)?.sql ?? "")).not.toMatch(/\bLIMIT\b/i);
    const validCountRequest = countRequests.at(-1)!;
    const pageTokenCountResponse = await request.post(
      getBackendUrl(`/spaces/${seed.spaceId}/sql/query/count`),
      {
        data: {
          ...validCountRequest,
          composition_count_snapshot: firstPage?.next,
        },
      },
    );
    expect(pageTokenCountResponse.status()).toBe(422);
    const tamperedSnapshotResponse = await request.post(
      getBackendUrl(`/spaces/${seed.spaceId}/sql/query/count`),
      {
        data: {
          ...validCountRequest,
          composition_count_snapshot: `${nextPage?.count_snapshot}x`,
        },
      },
    );
    expect(tamperedSnapshotResponse.status()).toBe(422);
    expect(
      await getSavedSqlSource(request, seed.spaceId, seed.tableSql.entryId),
    )
      .toBe(sourceBefore);
  });
});
