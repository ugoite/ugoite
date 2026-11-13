import {
  type APIRequestContext,
  expect,
  type Page,
  test,
} from "@playwright/test";
import { parse as parseYaml } from "yaml";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";
import {
  completeCompositionGraphYaml,
  type CompositionGraphSeed,
  saveCompositionGraph,
} from "./lib/composition-graph.ts";

type FormField = { sql_column?: string };
type FormSummary = {
  id?: string;
  name?: string;
  sql_relation?: string;
  fields?: Record<string, FormField>;
};
type ListedEntry = { id?: string; form?: string };
type EntryDetail = { id?: string; form?: string; revision_id?: string };
type UserEntryIdentity = { id: string; form: string; revision_id: string };
type SavedSqlSummary = { id?: string; revision_id?: string };
type SavedComposition = { composition_id: string; revision_id: string };
type CompositionRevision = {
  revision: { entry_id: string; revision_id: string };
  fields: Record<string, unknown>;
};
type CompositionEvaluation = {
  ok: boolean;
  query_results: Record<string, {
    value: {
      kind: string;
      columns?: Array<{ name: string }>;
      rows?: unknown[][];
    };
  }>;
};

const RESERVED_METADATA_FORMS = new Set([
  "SQL",
  "User",
  "UserGroup",
  "_ugoite_compositions",
]);

const sqlIdentifier = (value: string) => `"${value.replaceAll('"', '""')}"`;
const slug = (value: string) =>
  value.normalize("NFKD").toLowerCase()
    .replace(/\p{M}/gu, "").replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");

async function createSpace(
  request: APIRequestContext,
  prefix: string,
): Promise<string> {
  const suffix = crypto.randomUUID().slice(0, 8);
  const response = await request.post(getBackendUrl("/spaces"), {
    data: {
      slug: `${prefix}-${suffix}`,
      name: `${prefix} ${suffix}`,
    },
  });
  expect([200, 201]).toContain(response.status());
  const result = await response.json() as { space_uid?: string };
  expect(result.space_uid).toBeTruthy();
  return result.space_uid!;
}

async function createForm(
  request: APIRequestContext,
  spaceId: string,
  formName: string,
  fields: Record<string, { type: string; required: boolean }>,
  template: string,
): Promise<FormSummary> {
  const createResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: { name: formName, version: 1, fields, template },
    },
  );
  expect([200, 201]).toContain(createResponse.status());
  const listResponse = await request.get(
    getBackendUrl(`/spaces/${spaceId}/forms`),
  );
  expect(listResponse.ok()).toBe(true);
  const forms = await listResponse.json() as FormSummary[];
  const form = forms.find((candidate) => candidate.name === formName);
  expect(form?.id).toBeTruthy();
  expect(form?.sql_relation).toBeTruthy();
  return form!;
}

async function createEntry(
  request: APIRequestContext,
  spaceId: string,
  formName: string,
  fields: Record<string, unknown>,
): Promise<void> {
  const response = await request.post(
    getBackendUrl(`/spaces/${spaceId}/entries`),
    { data: { form: formName, fields } },
  );
  expect(response.status(), await response.text()).toBe(201);
}

async function snapshotUserEntryIdentities(
  request: APIRequestContext,
  spaceId: string,
): Promise<UserEntryIdentity[]> {
  const listResponse = await request.get(
    getBackendUrl(`/spaces/${spaceId}/entries?limit=1000&offset=0`),
  );
  expect(listResponse.ok()).toBe(true);
  const listedEntries = await listResponse.json() as ListedEntry[];
  const userEntries = listedEntries.filter((entry) =>
    !!entry.id && !!entry.form && !RESERVED_METADATA_FORMS.has(entry.form)
  );
  const identities = await Promise.all(userEntries.map(async (entry) => {
    const detailResponse = await request.get(
      getBackendUrl(`/spaces/${spaceId}/entries/${entry.id}`),
    );
    expect(detailResponse.ok()).toBe(true);
    const detail = await detailResponse.json() as EntryDetail;
    expect(detail.id).toBe(entry.id);
    expect(detail.form).toBe(entry.form);
    expect(detail.revision_id).toBeTruthy();
    return {
      id: detail.id!,
      form: detail.form!,
      revision_id: detail.revision_id!,
    };
  }));
  return identities.sort((left, right) => left.id.localeCompare(right.id));
}

async function userFormIds(
  request: APIRequestContext,
  spaceId: string,
): Promise<string[]> {
  const response = await request.get(
    getBackendUrl(`/spaces/${spaceId}/forms`),
  );
  expect(response.ok()).toBe(true);
  const forms = await response.json() as FormSummary[];
  return forms.filter((form) =>
    !!form.id && !!form.name && !RESERVED_METADATA_FORMS.has(form.name)
  ).map((form) => form.id!).sort();
}

async function savedSqlRevisions(
  request: APIRequestContext,
  spaceId: string,
): Promise<SavedSqlSummary[]> {
  const response = await request.get(
    getBackendUrl(`/spaces/${spaceId}/sql`),
  );
  expect(response.ok()).toBe(true);
  return (await response.json() as SavedSqlSummary[])
    .map(({ id, revision_id }) => ({ id, revision_id }))
    .sort((left, right) => String(left.id).localeCompare(String(right.id)));
}

function queryNode(
  id: string,
  sql: string,
  shape: "table" | "scalar",
  arguments_: Array<{ name: string; variable: string }> = [],
) {
  return {
    id,
    kind: "query" as const,
    returns: shape === "table" ? { kind: "table", columns: [] } : {
      kind: "scalar",
      value_type: { scalar_type: "integer", nullable: true },
    },
    query: {
      kind: "inline_sql",
      sql,
      arguments: arguments_,
      form_bindings: [],
      schema_fingerprint: "",
    },
  };
}

async function saveGraph(
  request: APIRequestContext,
  spaceId: string,
  graph: CompositionGraphSeed,
): Promise<SavedComposition> {
  const yaml = await completeCompositionGraphYaml(request, spaceId, graph);
  return await saveCompositionGraph(request, spaceId, yaml);
}

async function openReader(
  page: Page,
  spaceId: string,
  composition: SavedComposition,
): Promise<void> {
  await page.goto(
    getFrontendUrl(
      `/spaces/${spaceId}/compositions/${composition.composition_id}/${composition.revision_id}`,
    ),
    { waitUntil: "domcontentloaded" },
  );
  await expect(page).toHaveURL(
    new RegExp(
      `/spaces/${spaceId}/compositions/${composition.composition_id}/${composition.revision_id}$`,
    ),
  );
}

test.describe("Text-first Composition graph bindings", () => {
  test.beforeAll(async ({ request }) => await waitForServers(request));

  test("Composition keeps Table SQL inline with its Form identity and leaves Saved SQL and user Entries intact", async ({ page, request }) => {
    test.setTimeout(120_000);
    const spaceId = await createSpace(request, "composition-inline-table");
    const suffix = crypto.randomUUID().slice(0, 8);
    const formName = `RebindForm${suffix}`;
    const form = await createForm(
      request,
      spaceId,
      formName,
      {
        Item: { type: "string", required: true },
        Amount: { type: "integer", required: true },
      },
      `# ${formName}\n\n## Item\n\n## Amount\n`,
    );
    await createEntry(request, spaceId, formName, {
      Item: "First shared row",
      Amount: 11,
    });
    await createEntry(request, spaceId, formName, {
      Item: "Second shared row",
      Amount: 22,
    });

    const itemColumn = form.fields?.Item?.sql_column;
    const amountColumn = form.fields?.Amount?.sql_column;
    expect(itemColumn).toBeTruthy();
    expect(amountColumn).toBeTruthy();
    const inlineSql = `SELECT ${sqlIdentifier(itemColumn!)} AS item, ${
      sqlIdentifier(amountColumn!)
    } AS amount FROM ${sqlIdentifier(form.sql_relation!)} ORDER BY ${
      sqlIdentifier(amountColumn!)
    } ASC`;

    // The public Saved SQL API remains usable and independent. Composition
    // stores its own inline SQL instead of pinning this Saved SQL revision.
    const standaloneSqlResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql`),
      {
        data: {
          name: `Standalone ${suffix}`,
          kind: "user-query",
          sql: "SELECT 42 AS value",
          variables: [],
        },
      },
    );
    expect([200, 201]).toContain(standaloneSqlResponse.status());
    const standaloneSql = await standaloneSqlResponse.json() as {
      id: string;
      revision_id: string;
    };
    const savedSqlBefore = await savedSqlRevisions(request, spaceId);
    const entryIdentitiesBefore = await snapshotUserEntryIdentities(
      request,
      spaceId,
    );
    const userFormsBefore = await userFormIds(request, spaceId);

    const composition = await saveGraph(request, spaceId, {
      name: `Inline table ${suffix}`,
      variables: [queryNode("shared_rows", inlineSql, "table")],
      components: [
        { id: "rows_table", kind: "table", variable: "shared_rows" },
      ],
      rows: [{
        id: "main",
        items: [{ kind: "component", component: "rows_table" }],
      }],
    });

    const rawResponse = await request.get(
      getBackendUrl(
        `/spaces/${spaceId}/compositions/${composition.composition_id}/history/${composition.revision_id}`,
      ),
    );
    expect(rawResponse.ok()).toBe(true);
    const raw = await rawResponse.json() as CompositionRevision;
    expect(raw.revision).toEqual({
      entry_id: composition.composition_id,
      revision_id: composition.revision_id,
    });
    expect(typeof raw.fields["spec"]).toBe("string");
    const document = parseYaml(String(raw.fields["spec"])) as {
      spec: {
        variables: Array<{
          id: string;
          query: {
            kind: string;
            form_bindings: Array<{ relation: string; form_id: string }>;
          };
        }>;
        components: Array<{ kind: string }>;
      };
    };
    const sharedQuery = document.spec.variables.find((node) =>
      node.id === "shared_rows"
    );
    expect(sharedQuery?.query.kind).toBe("inline_sql");
    expect(sharedQuery?.query.form_bindings).toContainEqual({
      relation: form.sql_relation,
      form_id: form.id,
    });
    expect(document.spec.components.map((component) => component.kind))
      .toEqual(["table"]);
    expect(String(raw.fields["spec"])).not.toMatch(
      /saved_sql|metric|presentation/,
    );

    const evaluationResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/compositions/evaluate`),
      {
        data: {
          evaluation_id: crypto.randomUUID(),
          target: {
            kind: "saved_revision",
            composition_id: composition.composition_id,
            revision_id: composition.revision_id,
          },
          requested_queries: ["shared_rows"],
        },
      },
    );
    const evaluation = await evaluationResponse.json() as CompositionEvaluation;
    expect(evaluationResponse.ok(), JSON.stringify(evaluation)).toBe(true);
    expect(evaluation.ok).toBe(true);
    const table = evaluation.query_results.shared_rows.value;
    expect(table.kind).toBe("table");
    expect(table.columns?.map(({ name }) => name)).toEqual(["item", "amount"]);
    expect(table.rows).toHaveLength(2);
    expect(JSON.stringify(table.rows)).toContain("First shared row");
    expect(JSON.stringify(table.rows)).toContain("Second shared row");

    expect(await savedSqlRevisions(request, spaceId)).toEqual(savedSqlBefore);
    expect(savedSqlBefore).toContainEqual({
      id: standaloneSql.id,
      revision_id: standaloneSql.revision_id,
    });
    expect(await userFormIds(request, spaceId)).toEqual(userFormsBefore);
    expect(await snapshotUserEntryIdentities(request, spaceId))
      .toEqual(entryIdentitiesBefore);

    await openReader(page, spaceId, composition);
    const tableComponent = page.locator('[data-component-id="rows_table"]');
    const visibleTable = tableComponent.getByRole("table");
    await expect(visibleTable.locator("tbody tr")).toHaveCount(2);
    await expect(visibleTable).toContainText("First shared row");
    await expect(visibleTable).toContainText("Second shared row");
    await expect(
      page.getByRole("tablist", { name: "Studio mode" }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("group", { name: "Add to design" }),
    ).toHaveCount(0);
  });

  test("shared scalar queries update every Text reference after an Input change", async ({ page, request }) => {
    test.setTimeout(120_000);
    const spaceId = await createSpace(request, "composition-shared-text");
    const suffix = crypto.randomUUID().slice(0, 8);
    const formName = `SharedText${suffix}`;
    const form = await createForm(
      request,
      spaceId,
      formName,
      { Amount: { type: "integer", required: true } },
      `# ${formName}\n\n## Amount\n`,
    );
    await createEntry(request, spaceId, formName, { Amount: 7 });
    await createEntry(request, spaceId, formName, { Amount: 8 });

    const amountColumn = form.fields?.Amount?.sql_column;
    expect(amountColumn).toBeTruthy();
    const graph: CompositionGraphSeed = {
      name: `Shared Text ${suffix}`,
      variables: [
        {
          id: "minimum_amount",
          kind: "input",
          value_type: { scalar_type: "integer", nullable: false },
          required: true,
          default: { kind: "integer", value: 0 },
        },
        queryNode(
          "total_value",
          `SELECT SUM(${sqlIdentifier(amountColumn!)}) AS total FROM ${
            sqlIdentifier(form.sql_relation!)
          } WHERE ${sqlIdentifier(amountColumn!)} >= {{minimum_amount}}`,
          "scalar",
          [{ name: "minimum_amount", variable: "minimum_amount" }],
        ),
      ],
      components: [
        { id: "minimum_input", kind: "input", variable: "minimum_amount" },
        {
          id: "total_text",
          kind: "text",
          content: "Total: {{total_value}}",
          style: "body",
        },
        {
          id: "repeat_text",
          kind: "text",
          content: "Again: {{total_value}}",
          style: "body",
        },
      ],
      rows: [{
        id: "main",
        items: ["minimum_input", "total_text", "repeat_text"].map((
          component,
        ) => ({
          kind: "component" as const,
          component,
        })),
      }],
    };
    const composition = await saveGraph(request, spaceId, graph);
    const evaluations: Array<Record<string, unknown>> = [];
    page.on("request", (outgoing) => {
      if (
        outgoing.method() !== "POST" ||
        !new URL(outgoing.url()).pathname.endsWith("/compositions/evaluate")
      ) return;
      try {
        evaluations.push(outgoing.postDataJSON() as Record<string, unknown>);
      } catch {
        evaluations.push({});
      }
    });

    await openReader(page, spaceId, composition);
    const totalText = page.locator(
      '[data-component-id="total_text"] .compositionGraphText',
    );
    const repeatedText = page.locator(
      '[data-component-id="repeat_text"] .compositionGraphText',
    );
    await expect(totalText).toHaveText("Total: 15", { timeout: 60_000 });
    await expect(repeatedText).toHaveText("Again: 15", { timeout: 60_000 });
    expect(evaluations.at(-1)?.requested_queries).toEqual(["total_value"]);

    const previousEvaluationCount = evaluations.length;
    await page.getByLabel("minimum_amount", { exact: true }).fill("8");
    await expect(totalText).toHaveText("Total: 8", { timeout: 60_000 });
    await expect(repeatedText).toHaveText("Again: 8", { timeout: 60_000 });
    await expect.poll(() => evaluations.length).toBe(
      previousEvaluationCount + 1,
    );
    expect(evaluations.at(-1)?.requested_queries).toEqual(["total_value"]);
    await expect(
      page.getByRole("tablist", { name: "Studio mode" }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("textbox", { name: "Text", exact: true }),
    ).toHaveCount(0);
  });
});
