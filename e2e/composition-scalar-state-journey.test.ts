import {
  type APIRequestContext,
  type Browser,
  type BrowserContext,
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
import { parse as parseYaml } from "yaml";

type SeedState = {
  spaceId: string;
  compositionId: string;
  compositionRevisionId: string;
  compositionName: string;
};

type ScalarQueryDefinition = {
  id: string;
  sql: string;
  scalarType: "integer" | "float" | "decimal";
  nullable: boolean;
};

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

async function createSpace(request: APIRequestContext): Promise<string> {
  return await getDefaultSpaceId(request);
}

function compositionGraph(
  name: string,
  queries: ScalarQueryDefinition[],
): CompositionGraphSeed {
  const queryVariable = (query: ScalarQueryDefinition) => ({
    id: query.id,
    kind: "query" as const,
    returns: {
      kind: "scalar",
      value_type: {
        scalar_type: query.scalarType,
        nullable: query.nullable,
      },
    },
    query: {
      kind: "inline_sql",
      sql: query.sql,
      arguments: [],
      form_bindings: [],
      schema_fingerprint: "",
    },
  });
  return {
    name,
    variables: queries.map(queryVariable),
    components: queries.map((query) => ({
      id: query.id,
      kind: "text",
      content: `{{${query.id}}}`,
      style: "body",
    })),
    rows: [
      {
        id: "aggregates",
        items: queries.slice(0, 4).map((query) => ({
          kind: "component" as const,
          component: query.id,
        })),
      },
      {
        id: "scalar_states",
        items: queries.slice(4).map((query) => ({
          kind: "component" as const,
          component: query.id,
        })),
      },
    ],
  };
}

async function seedScalarStateComposition(
  request: APIRequestContext,
): Promise<{ aggregates: SeedState; emptyScalar: SeedState }> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `ScalarState${suffix}`;
  const compositionName = `Reactive scalar states ${suffix}`;
  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Amount\n`,
        fields: { Amount: { type: "double", required: false } },
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

  for (const fields of [{ Amount: 10 }, { Amount: 20 }, {}]) {
    const entryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      { data: { form: formName, fields } },
    );
    expect(entryResponse.status()).toBe(201);
  }

  const amount = sqlIdentifier(amountColumn!);
  const table = sqlIdentifier(relation!);
  const queries: ScalarQueryDefinition[] = [
    {
      id: "all_rows",
      sql: `SELECT COUNT(*) AS value FROM ${table}`,
      scalarType: "integer",
      nullable: false,
    },
    {
      id: "rows_with_amount",
      sql: `SELECT COUNT(${amount}) AS value FROM ${table}`,
      scalarType: "integer",
      nullable: false,
    },
    {
      id: "amount_total",
      sql: `SELECT SUM(${amount}) AS value FROM ${table}`,
      scalarType: "float",
      nullable: true,
    },
    {
      id: "amount_average",
      sql: `SELECT AVG(${amount}) AS value FROM ${table}`,
      scalarType: "float",
      nullable: true,
    },
    {
      id: "null_result",
      sql: `SELECT SUM(${amount}) AS value FROM ${table} WHERE ${amount} < 0`,
      scalarType: "float",
      nullable: true,
    },
    {
      id: "zero_result",
      sql: `SELECT COUNT(*) AS value FROM ${table} WHERE ${amount} < 0`,
      scalarType: "integer",
      nullable: false,
    },
  ];
  const yaml = await completeCompositionGraphYaml(
    request,
    spaceId,
    compositionGraph(compositionName, queries),
  );
  const aggregates = await saveCompositionSeed(
    request,
    spaceId,
    yaml,
    compositionName,
  );

  const zeroRowsSql = `SELECT ${amount} AS value FROM ${table} WHERE ${amount} = $minimum`;
  const multipleRowsSql = `SELECT ${amount} AS value FROM ${table} WHERE ${amount} <= $maximum`;
  const zeroRowsYaml = await completeCompositionGraphYaml(request, spaceId, {
    name: `Empty scalar after Input ${suffix}`,
    variables: [
      {
        id: "minimum",
        kind: "input",
        value_type: { scalar_type: "integer", nullable: false },
        required: true,
        default: { kind: "integer", value: 10 },
      },
      {
        id: "zero_rows",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "float", nullable: true },
        },
        query: {
          kind: "inline_sql",
          sql: zeroRowsSql,
          arguments: [{ name: "minimum", variable: "minimum" }],
          form_bindings: [],
          schema_fingerprint: "",
        },
      },
      {
        id: "maximum",
        kind: "input",
        value_type: { scalar_type: "integer", nullable: false },
        required: true,
        default: { kind: "integer", value: 10 },
      },
      {
        id: "multiple_rows",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "float", nullable: true },
        },
        query: {
          kind: "inline_sql",
          sql: multipleRowsSql,
          arguments: [{ name: "maximum", variable: "maximum" }],
          form_bindings: [],
          schema_fingerprint: "",
        },
      },
    ],
    components: [
      { id: "minimum_input", kind: "input", variable: "minimum" },
      { id: "maximum_input", kind: "input", variable: "maximum" },
      { id: "zero_rows", kind: "text", content: "{{zero_rows}}", style: "body" },
      { id: "multiple_rows", kind: "text", content: "{{multiple_rows}}", style: "body" },
    ],
    rows: [
      {
        id: "inputs",
        items: ["minimum_input", "maximum_input"].map((component) => ({
          kind: "component" as const,
          component,
        })),
      },
      {
        id: "scalar_states",
        items: ["zero_rows", "multiple_rows"].map((component) => ({
          kind: "component" as const,
          component,
        })),
      },
    ],
  });
  const emptyScalar = await saveCompositionSeed(
    request,
    spaceId,
    zeroRowsYaml,
    `Empty scalar after Input ${suffix}`,
  );
  return { aggregates, emptyScalar };
}

async function seedScalarCastFailureComposition(
  request: APIRequestContext,
): Promise<SeedState> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `ScalarCast${suffix}`;
  const compositionName = `Scalar cast failure ${suffix}`;
  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Amount\n`,
        fields: {
          Amount: { type: "double", required: false },
          "Numeric text": { type: "string", required: false },
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
  const relation = form?.sql_relation;
  const amountColumn = form?.fields?.Amount?.sql_column;
  const numericTextColumn = form?.fields?.["Numeric text"]?.sql_column;
  expect(relation).toBeTruthy();
  expect(amountColumn).toBeTruthy();
  expect(numericTextColumn).toBeTruthy();
  for (const fields of [
    { Amount: 10, "Numeric text": "10" },
    { Amount: 20, "Numeric text": "not-a-number" },
  ]) {
    const entryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      { data: { form: formName, fields } },
    );
    expect(entryResponse.status()).toBe(201);
  }

  const amount = sqlIdentifier(amountColumn!);
  const numericText = sqlIdentifier(numericTextColumn!);
  const table = sqlIdentifier(relation!);
  const castFailureSql = `SELECT CAST(${numericText} AS BIGINT) AS value ` +
    `FROM ${table} WHERE ${amount} = $maximum`;
  const yaml = await completeCompositionGraphYaml(request, spaceId, {
    name: compositionName,
    variables: [
      {
        id: "maximum",
        kind: "input",
        value_type: { scalar_type: "integer", nullable: false },
        required: true,
        default: { kind: "integer", value: 10 },
      },
      {
        id: "cast_failure",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: {
          kind: "inline_sql",
          sql: castFailureSql,
          arguments: [{ name: "maximum", variable: "maximum" }],
          form_bindings: [],
          schema_fingerprint: "",
        },
      },
    ],
    components: [
      { id: "maximum_input", kind: "input", variable: "maximum" },
      { id: "cast_failure", kind: "text", content: "{{cast_failure}}", style: "body" },
    ],
    rows: [{
      id: "values",
      items: ["maximum_input", "cast_failure"].map((component) => ({
        kind: "component" as const,
        component,
      })),
    }],
  });
  return await saveCompositionSeed(request, spaceId, yaml, compositionName);
}

async function saveCompositionSeed(
  request: APIRequestContext,
  spaceId: string,
  yaml: string,
  compositionName: string,
): Promise<SeedState> {
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
    compositionId: savedComposition.composition_id!,
    compositionRevisionId: savedComposition.revision_id!,
    compositionName,
  };
}

async function openSavedComposition(
  page: Page,
  seed: SeedState,
): Promise<void> {
  const path =
    `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/${seed.compositionRevisionId}`;
  await page.goto(getFrontendUrl(`/spaces/${seed.spaceId}/compositions`), {
    waitUntil: "domcontentloaded",
  });
  const link = page.locator(`a[href="${path}"]`);
  await expect(link).toBeVisible();
  await link.click();
  await page.waitForURL((url) => url.pathname === path);
  await expect(
    page.getByRole("heading", { level: 1, name: seed.compositionName }),
  ).toBeVisible();
}

function textOutput(page: Page, componentId: string) {
  return page.locator(
    `[data-component-id="${componentId}"] .compositionGraphText`,
  );
}

const scalarResultTimeoutMs = 30_000;

test("Composition preserves exact integer and decimal SQL results in Table and Text after reopening", async ({ page, request }) => {
  test.setTimeout(120_000);
  await waitForServers(request);
  const spaceId = await createSpace(request);
  const name = `Exact integer ${crypto.randomUUID().slice(0, 8)}`;
  const sql = "SELECT CAST('9223372036854775807' AS BIGINT) AS maximum, " +
    "CAST('-9223372036854775808' AS BIGINT) AS minimum, " +
    "CAST('12345678901234567890.0000000000000001' AS DECIMAL(38, 16)) " +
    "AS exact_decimal";
  const inline = {
    kind: "inline_sql",
    sql,
    arguments: [],
    form_bindings: [],
    schema_fingerprint: "",
  };
  const yaml = await completeCompositionGraphYaml(request, spaceId, {
    name,
    variables: [
      {
        id: "bounds",
        kind: "query",
        returns: { kind: "table", columns: [] },
        query: inline,
      },
      {
        id: "maximum",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: {
          ...inline,
          sql: "SELECT CAST('9223372036854775807' AS BIGINT) AS maximum",
        },
      },
      {
        id: "minimum",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: {
          ...inline,
          sql: "SELECT CAST('-9223372036854775808' AS BIGINT) AS minimum",
        },
      },
      {
        id: "exact_decimal",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "decimal", nullable: false },
        },
        query: {
          ...inline,
          sql: "SELECT CAST('12345678901234567890.0000000000000001' AS DECIMAL(38, 16)) AS exact_decimal",
        },
      },
    ],
    components: [
      { id: "table", kind: "table", variable: "bounds" },
      { id: "maximum_text", kind: "text", content: "{{maximum}}", style: "body" },
      { id: "minimum_text", kind: "text", content: "{{minimum}}", style: "body" },
      { id: "decimal_text", kind: "text", content: "{{exact_decimal}}", style: "body" },
    ],
    rows: [{
      id: "values",
      items: ["table", "maximum_text", "minimum_text", "decimal_text"].map((component) => ({
        kind: "component" as const,
        component,
      })),
    }],
  });
  const seed = await saveCompositionSeed(request, spaceId, yaml, name);
  await openSavedComposition(page, seed);
  for (let visit = 0; visit < 2; visit++) {
    const table = page.getByRole("table", { name: "bounds" });
    await expect(table).toContainText("9223372036854775807", {
      timeout: scalarResultTimeoutMs,
    });
    await expect(table).toContainText("-9223372036854775808", {
      timeout: scalarResultTimeoutMs,
    });
    await expect(textOutput(page, "maximum_text")).toHaveText(
      "9223372036854775807",
      { timeout: scalarResultTimeoutMs },
    );
    await expect(textOutput(page, "minimum_text")).toHaveText(
      "-9223372036854775808",
      { timeout: scalarResultTimeoutMs },
    );
    await expect(textOutput(page, "decimal_text")).toHaveText(
      "12345678901234567890.0000000000000001",
      { timeout: scalarResultTimeoutMs },
    );
    if (visit === 0) await page.reload({ waitUntil: "domcontentloaded" });
  }
});

async function openOwnerPage(
  browser: Browser,
  context: BrowserContext,
  seed: SeedState,
  edit = false,
) {
  const freshContext = await browser.newContext({
    storageState: await context.storageState(),
  });
  const page = await freshContext.newPage();
  await openSavedComposition(page, seed);
  if (edit) {
    await page.goto(
      getFrontendUrl(
        `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/${seed.compositionRevisionId}/edit`,
      ),
      { waitUntil: "domcontentloaded" },
    );
    await expect(page.locator(".compositionGraphDesignValue").first())
      .toBeVisible({ timeout: scalarResultTimeoutMs });
  }
  return { freshContext, page };
}

test.describe("Composition scalar state journey", () => {
  let seeds: {
    aggregates: SeedState;
    emptyScalar: SeedState;
    castFailure: SeedState;
  };

  test.beforeAll(async ({ request }) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    const scalarSeeds = await seedScalarStateComposition(request);
    seeds = {
      ...scalarSeeds,
      castFailure: await seedScalarCastFailureComposition(request),
    };
  });

  test("changing one Text reference preserves its shared query and sibling", async ({ browser, context, request }) => {
    test.setTimeout(120_000);
    const spaceId = await createSpace(request);
    const suffix = crypto.randomUUID().slice(0, 8);
    const compositionName = `Shared scalar text ${suffix}`;
    const inlineQuery = (sql: string) => ({
      kind: "inline_sql" as const,
      sql,
      arguments: [],
      form_bindings: [],
      schema_fingerprint: "",
    });
    const yaml = await completeCompositionGraphYaml(request, spaceId, {
      name: compositionName,
      variables: [
        {
          id: "shared_value",
          kind: "query",
          returns: {
            kind: "scalar",
            value_type: { scalar_type: "integer", nullable: false },
          },
          query: inlineQuery("SELECT CAST(10 AS BIGINT) AS value"),
        },
        {
          id: "alternative_value",
          kind: "query",
          returns: {
            kind: "scalar",
            value_type: { scalar_type: "integer", nullable: false },
          },
          query: inlineQuery("SELECT CAST(20 AS BIGINT) AS value"),
        },
      ],
      components: [
        { id: "text_one", kind: "text", content: "{{shared_value}}", style: "body" },
        { id: "text_two", kind: "text", content: "{{shared_value}}", style: "body" },
      ],
      rows: [{
        id: "values",
        items: ["text_one", "text_two"].map((component) => ({
          kind: "component" as const,
          component,
        })),
      }],
    });
    const initial = await saveCompositionSeed(
      request,
      spaceId,
      yaml,
      compositionName,
    );
    const initialRevisionResponse = await request.get(
      getBackendUrl(
        `/spaces/${spaceId}/compositions/${initial.compositionId}/history/${initial.compositionRevisionId}`,
      ),
    );
    expect(initialRevisionResponse.ok()).toBe(true);
    const initialRevision = await initialRevisionResponse.json() as {
      fields: Record<string, unknown>;
    };
    const initialSpec = parseYaml(String(initialRevision.fields["spec"])) as {
      spec?: {
        variables?: Array<{ id?: string }>;
        components?: Array<{ id?: string; content?: string }>;
      };
    };
    expect((initialSpec.spec?.variables ?? []).map(({ id }) => id).sort())
      .toEqual(["alternative_value", "shared_value"]);
    expect((initialSpec.spec?.components ?? []).map(({ id, content }) => [
      id,
      content,
    ])).toEqual([
      ["text_one", "{{shared_value}}"],
      ["text_two", "{{shared_value}}"],
    ]);

    const ownerContext = await browser.newContext({
      storageState: await context.storageState(),
    });
    let editedRevisionId = "";
    try {
      const page = await ownerContext.newPage();
      await page.goto(
        getFrontendUrl(
          `/spaces/${spaceId}/compositions/${initial.compositionId}/${initial.compositionRevisionId}/edit`,
        ),
        { waitUntil: "domcontentloaded" },
      );
      await expect(
        page.getByRole("group", { name: "Add to design", exact: true })
          .first(),
      ).toBeVisible({ timeout: 30_000 });
      const firstText = page.locator(
        '.compositionGraphDesignValue[data-component-id="text_one"] button.compositionGraphTextSelect',
      );
      await firstText.click();
      const firstTextEditor = page.locator(
        "#composition-text-editor-text_one",
      );
      await expect(firstTextEditor).toHaveValue("{{shared_value}}");
      await firstTextEditor.fill("{{alternative_value}}");
      await expect(firstTextEditor).toHaveValue("{{alternative_value}}");
      await expect(page.locator(
        '#composition-text-editor-text_two',
      )).toHaveValue("{{shared_value}}");

      await page.getByRole("button", { name: "Save", exact: true }).click();
      await expect(page).toHaveURL(
        new RegExp(
          `/spaces/${spaceId}/compositions/${initial.compositionId}/[^/]+$`,
        ),
        { timeout: 30_000 },
      );
      editedRevisionId = new URL(page.url()).pathname.split("/").at(-1)!;
      expect(editedRevisionId).not.toBe(initial.compositionRevisionId);

      const editedRevisionResponse = await request.get(
        getBackendUrl(
          `/spaces/${spaceId}/compositions/${initial.compositionId}/history/${editedRevisionId}`,
        ),
      );
      expect(editedRevisionResponse.ok()).toBe(true);
      const editedRevision = await editedRevisionResponse.json() as {
        fields: Record<string, unknown>;
      };
      const editedSpec = parseYaml(String(editedRevision.fields["spec"])) as {
        spec?: {
          variables?: Array<{
            id?: string;
            query?: { kind?: string; sql?: string };
          }>;
          components?: Array<{ id?: string; content?: string }>;
        };
      };
      const variables = editedSpec.spec?.variables ?? [];
      expect(variables.map(({ id }) => id).sort()).toEqual([
        "alternative_value",
        "shared_value",
      ]);
      expect(variables.find(({ id }) => id === "shared_value")?.query)
        .toMatchObject({
          kind: "inline_sql",
          sql: "SELECT CAST(10 AS BIGINT) AS value",
        });
      expect(variables.find(({ id }) => id === "alternative_value")?.query)
        .toMatchObject({
          kind: "inline_sql",
          sql: "SELECT CAST(20 AS BIGINT) AS value",
        });
      expect((editedSpec.spec?.components ?? []).map(({ id, content }) => [
        id,
        content,
      ])).toEqual([
        ["text_one", "{{alternative_value}}"],
        ["text_two", "{{shared_value}}"],
      ]);

      await openSavedComposition(page, {
        ...initial,
        compositionRevisionId: editedRevisionId,
      });
      await expect(textOutput(page, "text_one")).toHaveText("20", {
        timeout: scalarResultTimeoutMs,
      });
      await expect(textOutput(page, "text_two")).toHaveText("10", {
        timeout: scalarResultTimeoutMs,
      });
    } finally {
      await ownerContext.close();
    }
  });

  test("sql_aggregates_preserve_null_and_empty_semantics", async ({ browser, context }) => {
    test.setTimeout(120_000);
    const { freshContext, page } = await openOwnerPage(
      browser,
      context,
      seeds.aggregates,
    );
    try {
      await expect(textOutput(page, "all_rows")).toHaveText("3", {
        timeout: scalarResultTimeoutMs,
      });
      await expect(textOutput(page, "rows_with_amount")).toHaveText("2", {
        timeout: scalarResultTimeoutMs,
      });
      await expect(textOutput(page, "amount_total")).toHaveText("30", {
        timeout: scalarResultTimeoutMs,
      });
      await expect(textOutput(page, "amount_average")).toHaveText("15", {
        timeout: scalarResultTimeoutMs,
      });
    } finally {
      await freshContext.close();
    }
  });

  test("scalar_result_distinguishes_zero_rows_null_and_zero", async ({ browser, context }) => {
    test.setTimeout(120_000);
    const observations: Array<unknown> = [];
    for (const edit of [false, true]) {
      const { freshContext: aggregateContext, page: aggregatePage } =
        await openOwnerPage(browser, context, seeds.aggregates, edit);
      let aggregateStates: { null: string | null; zero: string | null };
      try {
        const nullOutput = textOutput(aggregatePage, "null_result");
        await expect(nullOutput).toHaveText(
          await aggregatePage.locator("html").getAttribute("lang").then((
            locale,
          ) => locale === "ja" ? "データなし" : "No data"),
          { timeout: scalarResultTimeoutMs },
        );
        const zeroOutput = textOutput(aggregatePage, "zero_result");
        await expect(zeroOutput).toHaveText("0", {
          timeout: scalarResultTimeoutMs,
        });
        await expect(nullOutput).not.toHaveText("0", {
          timeout: scalarResultTimeoutMs,
        });
        aggregateStates = {
          null: await nullOutput.textContent(),
          zero: await zeroOutput.textContent(),
        };
      } finally {
        await aggregateContext.close();
      }

      const { freshContext, page } = await openOwnerPage(
        browser,
        context,
        seeds.emptyScalar,
        edit,
      );
      try {
        const minimumInput = page.getByRole("textbox", {
          name: "minimum",
          exact: true,
        });
        await expect(minimumInput).toHaveValue("10");
        const zeroRowsOutput = textOutput(page, "zero_rows");
        await expect(zeroRowsOutput).toHaveText("10", {
          timeout: scalarResultTimeoutMs,
        });
        await minimumInput.fill("-1");
        const zeroRowsSection = page.getByRole("region", {
          name: "scalar_states",
          exact: true,
        });
        const zeroRowsAlert = zeroRowsSection.getByRole("alert");
        await expect(zeroRowsAlert).toContainText(
          await page.locator("html").getAttribute("lang").then((locale) =>
            locale === "ja"
              ? "クエリが値を返しませんでした。"
              : "The query returned no value."
          ),
          { timeout: scalarResultTimeoutMs },
        );
        const zeroRowsMessage = await zeroRowsAlert.textContent();
        const zeroRowsText = textOutput(page, "zero_rows");
        await expect(zeroRowsText).toBeVisible({
          timeout: scalarResultTimeoutMs,
        });
        const zeroRowsTextCount = await zeroRowsText.count();
        await minimumInput.fill("10");
        await expect(zeroRowsOutput).toHaveText("10", {
          timeout: scalarResultTimeoutMs,
        });

        const maximumInput = page.getByRole("textbox", {
          name: "maximum",
          exact: true,
        });
        await expect(maximumInput).toHaveValue("10");
        const multipleRowsOutput = textOutput(page, "multiple_rows");
        await expect(multipleRowsOutput).toHaveText("10", {
          timeout: scalarResultTimeoutMs,
        });
        const failedEvaluation = page.waitForResponse((response) => {
          if (
            response.request().method() !== "POST" ||
            !new URL(response.url()).pathname.endsWith("/compositions/evaluate")
          ) return false;
          return response.request().postDataJSON().variables?.maximum === 20;
        });
        await maximumInput.fill("20");
        const result = await (await failedEvaluation).json();
        expect(result.ok).toBe(false);
        const multipleRowsDiagnostic = result.query_diagnostics.find((
          diagnostic: { variable_id: string },
        ) => diagnostic.variable_id === "multiple_rows");
        expect(
          multipleRowsDiagnostic?.result_diagnostic?.result_diagnostic?.code,
        )
          .toBe("scalar_multiple_rows");
        expect(result.query_diagnostics).toEqual(expect.arrayContaining([
          expect.objectContaining({
            variable_id: "multiple_rows",
            result_diagnostic: expect.objectContaining({
              result_diagnostic: expect.objectContaining({
                code: "scalar_multiple_rows",
              }),
            }),
          }),
        ]));
        const multipleRowsSection = page.getByRole("region", {
          name: "scalar_states",
          exact: true,
        });
        const multipleRowsAlert = multipleRowsSection.getByRole("alert");
        await expect(multipleRowsAlert).toContainText(
          await page.locator("html").getAttribute("lang").then((locale) =>
            locale === "ja"
              ? "クエリが複数の値を返しました。"
              : "The query returned more than one value."
          ),
          { timeout: scalarResultTimeoutMs },
        );
        const multipleRowsMessage = await multipleRowsAlert.textContent();
        const multipleRowsText = textOutput(page, "multiple_rows");
        await expect(multipleRowsText).toBeVisible({
          timeout: scalarResultTimeoutMs,
        });
        const multipleRowsTextCount = await multipleRowsText.count();
        await maximumInput.fill("10");
        await expect(multipleRowsOutput).toHaveText("10", {
          timeout: scalarResultTimeoutMs,
        });
        observations.push({
          aggregate: aggregateStates,
          zeroRows: {
            initial: "10",
            emptyMessage: zeroRowsMessage,
            textCountAfterEmpty: zeroRowsTextCount,
          },
          multipleRows: {
            initial: "10",
            diagnostic: multipleRowsDiagnostic.result_diagnostic
              .result_diagnostic.code,
            message: multipleRowsMessage,
            textCountAfterFailure: multipleRowsTextCount,
          },
        });
      } finally {
        await freshContext.close();
      }
    }
    expect(observations[1]).toEqual(observations[0]);
  });

  test("scalar_cast_execution_failure_remains_distinct_from_source_unavailability", async ({ browser, context }) => {
    test.setTimeout(120_000);
    for (const edit of [false, true]) {
      const { freshContext, page } = await openOwnerPage(
        browser,
        context,
        seeds.castFailure,
        edit,
      );
      try {
        const maximumInput = page.getByRole("textbox", {
          name: "maximum",
          exact: true,
        });
        const castOutput = textOutput(page, "cast_failure");
        await expect(maximumInput).toHaveValue("10");
        await expect(castOutput).toHaveText("10", {
          timeout: scalarResultTimeoutMs,
        });

        const failedEvaluation = page.waitForResponse((response) => {
          if (
            response.request().method() !== "POST" ||
            !new URL(response.url()).pathname.endsWith("/compositions/evaluate")
          ) return false;
          return response.request().postDataJSON().variables?.maximum === 20;
        });
        await maximumInput.fill("20");
        const failureResponse = await failedEvaluation;
        expect(failureResponse.status()).toBe(422);
        const failureBody = await failureResponse.json();
        expect(failureBody.code).toBe("INVALID_INPUT");
        expect(failureBody.message).toContain("SQL query execution failed");
        expect(JSON.stringify(failureBody)).not.toContain("query_unavailable");

        const errorAlert = page.getByRole("alert").first();
        await expect(errorAlert).toBeVisible({
          timeout: scalarResultTimeoutMs,
        });
        const alertText = (await page.getByRole("alert").allTextContents())
          .join(" ");
        expect(alertText).not.toContain("This source is unavailable");
        expect(alertText).not.toContain("データソースを利用できない");
        await expect(castOutput.getByRole("alert")).toBeVisible({
          timeout: scalarResultTimeoutMs,
        });

        const repairedEvaluation = page.waitForResponse((response) => {
          if (
            response.request().method() !== "POST" ||
            !new URL(response.url()).pathname.endsWith("/compositions/evaluate")
          ) return false;
          return response.request().postDataJSON().variables?.maximum === 10;
        });
        await maximumInput.fill("10");
        const repairedResponse = await repairedEvaluation;
        expect(repairedResponse.ok()).toBe(true);
        await expect(castOutput).toHaveText("10", {
          timeout: scalarResultTimeoutMs,
        });
      } finally {
        await freshContext.close();
      }
    }
  });
});
