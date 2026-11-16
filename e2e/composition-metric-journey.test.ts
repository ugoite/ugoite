import { type APIRequestContext, expect, test } from "@playwright/test";
import {
  getBackendUrl,
  getDefaultSpaceId,
  waitForServers,
} from "./lib/client.ts";
import {
  completeCompositionGraphYaml,
  type CompositionGraphSeed,
} from "./lib/composition-graph.ts";

const SCALAR_VALUE = 40;
const SECOND_AMOUNT = 2;
type EvaluationBody = {
  ok: boolean;
  composition_revision?: { entry_id: string; revision_id: string };
  query_results: Record<string, {
    value: { kind: string; rows?: unknown[][]; value?: unknown };
    has_more: boolean;
  }>;
  query_diagnostics: Array<{
    code?: string;
    variable_id: string;
    result_diagnostic?: { result_diagnostic?: { code?: string } };
  }>;
};

type SeedState = {
  spaceId: string;
  formName: string;
  compositionId: string;
  revisionId: string;
};

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

async function createSpace(request: APIRequestContext): Promise<string> {
  return await getDefaultSpaceId(request);
}

async function seedScalarTextComposition(
  request: APIRequestContext,
): Promise<SeedState> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `ScalarText${suffix}`;
  const compositionName = `Scalar text ${suffix}`;

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
  const forms = await formsResponse.json() as Array<{
    name?: string;
    sql_relation?: string;
    fields?: Record<string, { sql_column?: string }>;
  }>;
  const form = forms.find((candidate) => candidate.name === formName);
  const relation = form?.sql_relation;
  const dateColumn = form?.fields?.Date?.sql_column;
  const merchantColumn = form?.fields?.Merchant?.sql_column;
  const amountColumn = form?.fields?.Amount?.sql_column;
  expect(relation).toBeTruthy();
  expect(dateColumn).toBeTruthy();
  expect(merchantColumn).toBeTruthy();
  expect(amountColumn).toBeTruthy();

  const entryResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/entries`),
    {
      data: {
        form: formName,
        fields: {
          Date: "2026-04-10",
          Merchant: "Scalar-One",
          Amount: SCALAR_VALUE,
        },
      },
    },
  );
  expect(entryResponse.status()).toBe(201);

  const inlineQuery = (sql: string) => ({
    kind: "inline_sql",
    sql,
    arguments: [],
    form_bindings: [],
    schema_fingerprint: "",
  });

  const graph: CompositionGraphSeed = {
    name: compositionName,
    variables: [
      {
        id: "total_value",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "float", nullable: false },
        },
        query: {
          ...inlineQuery(
            `SELECT ${sqlIdentifier(amountColumn!)} AS total FROM ${sqlIdentifier(relation!)} ORDER BY ${sqlIdentifier(merchantColumn!)} ASC`,
          ),
        },
      },
      {
        id: "scalar_rows",
        kind: "query",
        returns: { kind: "table", columns: [] },
        query: {
          ...inlineQuery(
            `SELECT ${sqlIdentifier(amountColumn!)} AS amount, ${sqlIdentifier(merchantColumn!)} AS merchant FROM ${sqlIdentifier(relation!)} ORDER BY ${sqlIdentifier(merchantColumn!)} ASC`,
          ),
        },
      },
    ],
    components: [
      { id: "total", kind: "text", content: "{{total_value}}", style: "body" },
      { id: "rows", kind: "table", variable: "scalar_rows" },
    ],
    rows: [{
      id: "main",
      items: [
        { kind: "component", component: "total" },
        { kind: "component", component: "rows" },
      ],
    }],
  };
  const yaml = await completeCompositionGraphYaml(request, spaceId, graph);
  const saveResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions`),
    {
      headers: { "Idempotency-Key": crypto.randomUUID() },
      data: { yaml },
    },
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
    formName,
    compositionId: saved.composition_id!,
    revisionId: saved.revision_id!,
  };
}

test.describe("Composition scalar Text Journey", () => {
  let seed: SeedState;

  test.beforeAll(async ({ request }) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    seed = await seedScalarTextComposition(request);
  });

  test("API evaluates an exact Composition revision and reports scalar cardinality failures", async ({ request }) => {
    test.setTimeout(120_000);
    const evaluate = () =>
      request.post(
        getBackendUrl("/spaces/" + seed.spaceId + "/compositions/evaluate"),
        {
          data: {
            evaluation_id: crypto.randomUUID(),
            target: {
              kind: "saved_revision",
              composition_id: seed.compositionId,
              revision_id: seed.revisionId,
            },
            requested_queries: ["total_value", "scalar_rows"],
          },
        },
      );

    const firstResponse = await evaluate();
    const firstBody = await firstResponse.json() as EvaluationBody;
    expect(firstResponse.ok(), JSON.stringify(firstBody)).toBe(true);
    expect(firstBody.ok).toBe(true);
    expect(firstBody.composition_revision).toEqual({
      entry_id: seed.compositionId,
      revision_id: seed.revisionId,
    });
    expect(firstBody.query_results.total_value.value.kind).toBe("scalar");
    expect(JSON.stringify(firstBody.query_results.total_value.value))
      .toContain(String(SCALAR_VALUE));
    expect(firstBody.query_results.scalar_rows.value.rows).toHaveLength(1);
    expect(JSON.stringify(firstBody.query_results.scalar_rows.value.rows))
      .toContain("Scalar-One");

    const secondEntryResponse = await request.post(
      getBackendUrl("/spaces/" + seed.spaceId + "/entries"),
      {
        data: {
          form: seed.formName,
          fields: {
            Date: "2026-04-11",
            Merchant: "Scalar-Two",
            Amount: SECOND_AMOUNT,
          },
        },
      },
    );
    expect(secondEntryResponse.status()).toBe(201);

    const secondResponse = await evaluate();
    const secondBody = await secondResponse.json() as EvaluationBody;
    expect(secondResponse.ok(), JSON.stringify(secondBody)).toBe(true);
    expect(secondBody.ok).toBe(false);
    expect(secondBody.query_results).toEqual({});
    expect(secondBody.query_diagnostics).toContainEqual(
      expect.objectContaining({
        code: "result_validation_failed",
        variable_id: "total_value",
        result_diagnostic: expect.objectContaining({
          result_diagnostic: expect.objectContaining({
            code: "scalar_multiple_rows",
          }),
        }),
      }),
    );
    expect(secondBody.query_diagnostics).toContainEqual(
      expect.objectContaining({
        code: "evaluation_aborted",
        variable_id: "scalar_rows",
      }),
    );
  });
});
