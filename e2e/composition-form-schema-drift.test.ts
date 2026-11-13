import { expect, test } from "@playwright/test";
import { parse as parseYaml } from "yaml";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";
import {
  type CompositionFormSeedField,
  expectedCompositionFormSeedSql,
  normalizeCompositionFormSeedSql,
} from "./lib/composition-form.ts";

type FormSummary = {
  id?: string;
  name?: string;
  fields?: Record<string, CompositionFormSeedField>;
};

type SavedRevision = {
  revision: { entry_id: string; revision_id: string };
  fields: Record<string, unknown>;
};

function revisionFromUrl(url: string) {
  const match = url.match(/\/compositions\/([^/]+)\/([^/]+)$/);
  expect(match).toBeTruthy();
  return { compositionId: match![1], revisionId: match![2] };
}

test("Composition keeps a saved query executable after an unused Form field is added", async ({ browser, context, request }) => {
  await waitForServers(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const spaceName = `Form schema drift ${suffix}`;
  const spaceResponse = await request.post(getBackendUrl("/spaces"), {
    data: { slug: `form-schema-drift-${suffix}`, name: spaceName },
  });
  expect([200, 201]).toContain(spaceResponse.status());
  const spaceBody = await spaceResponse.json() as { space_uid?: string };
  expect(spaceBody.space_uid).toBeTruthy();
  const spaceId = spaceBody.space_uid!;

  const formName = `SchemaDrift${suffix}`;
  const formCreate = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Title\n\n## Summary\n`,
        fields: {
          Title: { id: 100, type: "string", required: false },
          Summary: { id: 101, type: "string", required: false },
        },
      },
    },
  );
  expect([200, 201]).toContain(formCreate.status());
  const formId = formCreate.headers()["x-ugoite-form-id"];
  expect(formId).toBeTruthy();

  const initialEntry = await request.post(
    getBackendUrl(`/spaces/${spaceId}/entries`),
    {
      data: {
        form: formName,
        fields: {
          Title: "before schema change",
          Summary: "original summary",
        },
      },
    },
  );
  expect(initialEntry.status()).toBe(201);

  const formsBeforeResponse = await request.get(
    getBackendUrl(`/spaces/${spaceId}/forms`),
  );
  expect(formsBeforeResponse.ok()).toBe(true);
  const formsBefore = await formsBeforeResponse.json() as FormSummary[];
  const formBefore = formsBefore.find((form) => form.name === formName);
  expect(formBefore?.id).toBe(formId);
  expect(formBefore?.fields?.Title?.sql_column).toBeTruthy();
  expect(formBefore?.fields?.Summary?.sql_column).toBeTruthy();
  const formSlug = formName.toLowerCase().replace(/[^a-z0-9]+/g, "_");
  const variableId = `${formSlug}_rows`;
  const compositionName = `Schema drift repair ${suffix}`;

  const ownerContext = await browser.newContext({
    storageState: await context.storageState(),
  });
  try {
    const page = await ownerContext.newPage();
    await page.goto(
      getFrontendUrl(`/spaces/${spaceId}/compositions/new`),
      { waitUntil: "domcontentloaded" },
    );
    await expect(page.getByPlaceholder("Composition name")).toBeVisible();
    await page.getByLabel("Name", { exact: true }).fill(compositionName);
    const palette = page.getByRole("group", { name: "Add to design" });
    await palette.getByRole("button", { name: "Table", exact: true }).click();
    const picker = page.getByRole("dialog", { name: "Table", exact: true });
    await picker.getByRole("tab", { name: "Forms", exact: true }).click();
    await picker.getByRole("button", { name: formName, exact: true }).click();
    await expect(picker).toHaveCount(0);

    const tableName = `${variableId} results`;
    const initialTable = page.getByRole("table", { name: tableName });
    await expect(initialTable).toBeVisible();
    await page.getByRole("button", {
      name: `table, ${variableId}`,
      exact: true,
    }).click();
    const tableInspector = page.locator(".compositionGraphInspector");
    await expect(
      tableInspector.getByRole("heading", { name: "Columns to display" }),
    ).toBeVisible();
    await expect(
      tableInspector.getByRole("textbox", { name: "Display label for Title" }),
    ).toHaveValue("Title");
    await expect(
      tableInspector.getByRole("textbox", {
        name: "Display label for Summary",
      }),
    ).toHaveValue("Summary");
    await tableInspector.getByRole("button", {
      name: "Remove Summary from the Table",
    }).click();
    await expect(initialTable.locator("thead")).toContainText("Title");
    await expect(initialTable.locator("thead")).not.toContainText("Summary");
    await expect(initialTable.locator("tbody")).toContainText(
      "before schema change",
    );
    await expect(initialTable.locator("tbody")).not.toContainText(
      "original summary",
    );
    await page.getByRole("button", { name: "Save", exact: true }).click();
    await expect(page).toHaveURL(
      new RegExp(`/spaces/${spaceId}/compositions/[^/]+/[^/]+$`),
    );
    const original = revisionFromUrl(page.url());
    const originalRevisionPath = getBackendUrl(
      `/spaces/${spaceId}/compositions/${original.compositionId}/history/${original.revisionId}`,
    );
    const originalRevisionResponse = await request.get(originalRevisionPath);
    expect(originalRevisionResponse.ok()).toBe(true);
    const originalRevision = await originalRevisionResponse
      .json() as SavedRevision;
    expect(originalRevision.revision.revision_id).toBe(original.revisionId);
    const originalDocument = parseYaml(
      String(originalRevision.fields["spec"]),
    ) as {
      spec?: {
        variables?: Array<{
          id?: string;
          query?: { schema_fingerprint?: string; sql?: string };
        }>;
        components?: Array<{
          kind?: string;
          variable?: string;
          presentation?: {
            columns?: Array<{ column?: string; label?: string }>;
          };
        }>;
      };
    };
    const originalQuery = originalDocument.spec?.variables?.find((variable) =>
      variable.id === variableId
    );
    expect(originalQuery?.query?.schema_fingerprint)
      .toMatch(/^[0-9a-f]{64}$/);
    expect(
      normalizeCompositionFormSeedSql(originalQuery?.query?.sql ?? ""),
    ).toBe(expectedCompositionFormSeedSql(formName, formBefore?.fields));
    const originalTable = originalDocument.spec?.components?.find(
      (component) =>
        component.kind === "table" &&
        component.variable === variableId,
    );
    expect(originalTable?.presentation?.columns).toEqual([
      { column: "Title", label: "Title" },
    ]);

    // This API call represents the external event: the same Form gains one
    // optional field while the browser journey remains responsible for repair.
    const formEvolution = await request.post(
      getBackendUrl(`/spaces/${spaceId}/forms`),
      {
        data: {
          id: formId,
          name: formName,
          fields: {
            Title: { id: 100, type: "string", required: false },
            Summary: { id: 101, type: "string", required: false },
            Notes: { id: 102, type: "string", required: false },
          },
        },
      },
    );
    expect([200, 201]).toContain(formEvolution.status());
    expect(formEvolution.headers()["x-ugoite-form-id"]).toBe(formId);
    expect(formEvolution.headers()["x-ugoite-form-version"]).toBe("2");
    const formsAfterResponse = await request.get(
      getBackendUrl(`/spaces/${spaceId}/forms`),
    );
    expect(formsAfterResponse.ok()).toBe(true);
    const formsAfter = await formsAfterResponse.json() as FormSummary[];
    const formAfter = formsAfter.find((form) => form.id === formId);
    expect(formAfter?.fields?.Notes?.sql_column).toBeTruthy();
    const evolvedSql = expectedCompositionFormSeedSql(
      formName,
      formAfter?.fields,
    );
    const evolvedEntry = await request.post(
      getBackendUrl(`/spaces/${spaceId}/entries`),
      {
        data: {
          form: formName,
          fields: {
            Title: "after schema change",
            Summary: "current summary",
            Notes: "current Form descriptor",
          },
        },
      },
    );
    expect(evolvedEntry.status()).toBe(201);

    const oldRuntimePath =
      `/spaces/${spaceId}/compositions/${original.compositionId}/${original.revisionId}`;
    const oldEvaluationResponse = page.waitForResponse((response) =>
      new URL(response.url()).pathname.endsWith("/compositions/evaluate") &&
      response.request().method() === "POST"
    );
    await page.goto(getFrontendUrl(oldRuntimePath), {
      waitUntil: "domcontentloaded",
    });
    const oldEvaluation = await (await oldEvaluationResponse).json() as {
      ok?: boolean;
      query_results?: Record<string, unknown>;
      diagnostics?: Array<{ variable_id?: string; code?: string }>;
    };
    expect(oldEvaluation.ok).toBe(true);
    expect(oldEvaluation.query_results).toHaveProperty(variableId);
    const savedTable = page.getByRole("table", { name: tableName });
    await expect(savedTable).toBeVisible();
    await expect(savedTable.locator("tbody")).toContainText(
      "before schema change",
    );
    await expect(savedTable.locator("tbody")).toContainText(
      "after schema change",
    );
    await expect(savedTable.locator("thead")).toContainText("Title");
    await expect(savedTable.locator("thead")).not.toContainText("Notes");

    await page.goto(
      getFrontendUrl(`${oldRuntimePath}/edit`),
      { waitUntil: "domcontentloaded" },
    );
    await expect(
      page.getByRole("group", { name: "Add to design", exact: true }).first(),
    ).toBeVisible({ timeout: 30_000 });
    await expect(page.getByRole("tab", { name: "Design", exact: true }))
      .toHaveAttribute("aria-selected", "true");
    const studioTabs = page.getByRole("tablist", { name: "Studio mode" });
    await studioTabs.getByRole("tab", { name: "Variables", exact: true })
      .click();
    await page.getByRole("button", { name: variableId, exact: true }).click();
    // Form seeds use explicit projections, so repairing against the evolved
    // schema includes the new field in the authored SQL.
    await page.getByRole("textbox", { name: "SQL", exact: true })
      .fill(evolvedSql);
    const currentSchemaPreviewResponse = page.waitForResponse((response) =>
      new URL(response.url()).pathname.endsWith("/compositions/evaluate") &&
      response.request().method() === "POST"
    );
    await page.getByRole("button", { name: "Run Query", exact: true }).click();
    const currentSchemaPreview = await (await currentSchemaPreviewResponse)
      .json() as {
        ok?: boolean;
        queries?: Array<{
          variable_id?: string;
          schema_fingerprint?: string;
          returns?: { columns?: Array<{ name?: string }> };
        }>;
        query_results?: Record<string, {
          value?: { columns?: Array<{ name?: string }> };
        }>;
      };
    expect(currentSchemaPreview.ok).toBe(true);
    const currentSchemaQuery = currentSchemaPreview.queries?.find((query) =>
      query.variable_id === variableId
    );
    expect(currentSchemaQuery?.schema_fingerprint)
      .toMatch(/^[0-9a-f]{64}$/);
    expect(currentSchemaQuery?.schema_fingerprint)
      .not.toBe(originalQuery?.query?.schema_fingerprint);
    expect(currentSchemaQuery?.returns?.columns).toContainEqual(
      expect.objectContaining({ name: "Notes" }),
    );
    expect(
      currentSchemaPreview.query_results?.[variableId]?.value?.columns?.map((
        column,
      ) => column.name),
    )
      .toContain("Notes");
    const authoringPreview = page.getByRole("table").first();
    await expect(authoringPreview).toContainText("Notes");
    await expect(authoringPreview).toContainText("current Form descriptor");
    await studioTabs.getByRole("tab", { name: "Design", exact: true }).click();
    const repairedPreviewTable = page.getByRole("table", { name: tableName });
    await expect(repairedPreviewTable).toBeVisible();

    const savedRevisionEvaluations: Array<{
      revisionId: string;
      response: Awaited<ReturnType<typeof page.waitForResponse>>;
    }> = [];
    page.on("response", (response) => {
      if (
        response.request().method() !== "POST" ||
        !new URL(response.url()).pathname.endsWith("/compositions/evaluate")
      ) return;
      const body = response.request().postDataJSON() as {
        target?: { kind?: string; revision_id?: string };
      } | null;
      if (
        body?.target?.kind === "saved_revision" &&
        body.target.revision_id
      ) {
        savedRevisionEvaluations.push({
          revisionId: body.target.revision_id,
          response,
        });
      }
    });
    await page.getByRole("button", { name: "Save", exact: true }).click();
    await expect(page).toHaveURL(
      new RegExp(
        `/spaces/${spaceId}/compositions/${original.compositionId}/[^/]+$`,
      ),
    );
    const repaired = revisionFromUrl(page.url());
    expect(repaired.compositionId).toBe(original.compositionId);
    expect(repaired.revisionId).not.toBe(original.revisionId);
    await expect.poll(() =>
      savedRevisionEvaluations.some(({ revisionId }) =>
        revisionId === repaired.revisionId
      )
    ).toBe(true);
    const repairedEvaluationResponse = savedRevisionEvaluations.find((entry) =>
      entry.revisionId === repaired.revisionId
    )!.response;
    const repairedEvaluation = await repairedEvaluationResponse.json() as {
      ok?: boolean;
      composition_revision?: { revision_id?: string };
      queries?: Array<{
        variable_id?: string;
        schema_fingerprint?: string;
        returns?: { columns?: Array<{ name?: string }> };
      }>;
      query_results?: Record<string, {
        value?: {
          kind?: string;
          columns?: Array<{ name?: string }>;
          rows?: Array<Array<{ value?: unknown }>>;
        };
      }>;
    };
    expect(repairedEvaluation.ok).toBe(true);
    expect(repairedEvaluation.composition_revision?.revision_id)
      .toBe(repaired.revisionId);
    const repairedRows = repairedEvaluation.query_results?.[variableId]?.value;
    expect(repairedRows?.kind).toBe("table");
    expect(repairedRows?.columns).toContainEqual(
      expect.objectContaining({ name: "Notes" }),
    );
    expect(JSON.stringify(repairedRows?.rows)).toContain(
      "current Form descriptor",
    );
    expect(JSON.stringify(repairedRows?.rows)).toContain("after schema change");
    const repairedRuntimeQuery = repairedEvaluation.queries?.find((query) =>
      query.variable_id === variableId
    );
    expect(repairedRuntimeQuery?.schema_fingerprint)
      .toBe(currentSchemaQuery?.schema_fingerprint);
    expect(repairedRuntimeQuery?.returns?.columns).toContainEqual(
      expect.objectContaining({ name: "Notes" }),
    );

    const repairedRevisionResponse = await request.get(
      getBackendUrl(
        `/spaces/${spaceId}/compositions/${repaired.compositionId}/history/${repaired.revisionId}`,
      ),
    );
    expect(repairedRevisionResponse.ok()).toBe(true);
    const repairedRevision = await repairedRevisionResponse
      .json() as SavedRevision;
    const repairedDocument = parseYaml(
      String(repairedRevision.fields["spec"]),
    ) as {
      spec?: {
        variables?: Array<{
          id?: string;
          query?: { schema_fingerprint?: string; sql?: string };
          returns?: { columns?: Array<{ name?: string }> };
        }>;
        components?: Array<{
          kind?: string;
          variable?: string;
          presentation?: {
            columns?: Array<{ column?: string; label?: string }>;
          };
        }>;
      };
    };
    const repairedQuery = repairedDocument.spec?.variables?.find((variable) =>
      variable.id === variableId
    );
    expect(repairedQuery?.query?.schema_fingerprint)
      .toMatch(/^[0-9a-f]{64}$/);
    expect(repairedQuery?.query?.schema_fingerprint)
      .toBe(currentSchemaQuery?.schema_fingerprint);
    expect(repairedQuery?.query?.schema_fingerprint)
      .not.toBe(originalQuery?.query?.schema_fingerprint);
    expect(
      normalizeCompositionFormSeedSql(repairedQuery?.query?.sql ?? ""),
    ).toBe(evolvedSql);
    expect(repairedQuery?.returns?.columns).toContainEqual(
      expect.objectContaining({ name: "Notes" }),
    );
    const repairedTable = repairedDocument.spec?.components?.find(
      (component) =>
        component.kind === "table" &&
        component.variable === variableId,
    );
    expect(repairedTable?.presentation?.columns).toEqual([
      { column: "Title", label: "Title" },
    ]);

    const newRuntimeTable = page.getByRole("table", { name: tableName });
    await expect(newRuntimeTable).toBeVisible();
    await expect(newRuntimeTable.locator("tbody")).toContainText(
      "after schema change",
    );
    await expect(newRuntimeTable.locator("thead")).not.toContainText("Summary");
    await expect(newRuntimeTable.locator("thead")).not.toContainText("Notes");
    await expect(newRuntimeTable.locator("tbody")).not.toContainText(
      "original summary",
    );
    await expect(newRuntimeTable.locator("tbody")).not.toContainText(
      "current summary",
    );
    await expect(newRuntimeTable.locator("tbody")).not.toContainText(
      "current Form descriptor",
    );
    const renderedTableHeaders = await newRuntimeTable.locator("thead")
      .innerText();
    expect(renderedTableHeaders.trim()).toBe("Title");

    const originalRevisionAfterResponse = await request.get(
      originalRevisionPath,
    );
    expect(originalRevisionAfterResponse.ok()).toBe(true);
    const originalRevisionAfter = await originalRevisionAfterResponse
      .json() as SavedRevision;
    expect(originalRevisionAfter).toEqual(originalRevision);
    const historyResponse = await request.get(
      getBackendUrl(
        `/spaces/${spaceId}/compositions/${original.compositionId}/history?limit=10&offset=0`,
      ),
    );
    expect(historyResponse.ok()).toBe(true);
    const history = await historyResponse.json() as {
      total?: number;
      revisions?: Array<{ revision: { revision_id: string } }>;
    };
    expect(history.total).toBe(2);
    expect(
      history.revisions?.map(({ revision }) =>
        revision.revision_id
      ),
    )
      .toContain(original.revisionId);
    expect(history.revisions?.map(({ revision }) => revision.revision_id))
      .toContain(repaired.revisionId);

    const oldEvaluationAfterResponse = page.waitForResponse((response) =>
      new URL(response.url()).pathname.endsWith("/compositions/evaluate") &&
      response.request().method() === "POST"
    );
    await page.goto(getFrontendUrl(oldRuntimePath), {
      waitUntil: "domcontentloaded",
    });
    const oldAfterRepair = await (await oldEvaluationAfterResponse)
      .json() as {
        ok?: boolean;
        query_results?: Record<string, unknown>;
      };
    expect(oldAfterRepair.ok).toBe(true);
    expect(oldAfterRepair.query_results).toHaveProperty(variableId);
    await expect(page.getByRole("table", { name: tableName })).toBeVisible();

    const buildInfoResponse = await request.get(
      getFrontendUrl("/build-info.json"),
    );
    expect(buildInfoResponse.ok()).toBe(true);
    const buildInfo = await buildInfoResponse.json() as { source_sha?: string };
    const sourceSha = Deno.env.get("UGOITE_SOURCE_SHA");
    expect(buildInfo.source_sha).toBe(sourceSha);
    console.log(
      "COMPOSITION_FORM_SCHEMA_DRIFT_EVIDENCE " + JSON.stringify({
        sourceSha,
        formId,
        formVersion: formEvolution.headers()["x-ugoite-form-version"],
        compositionId: original.compositionId,
        originalRevisionId: original.revisionId,
        repairedRevisionId: repaired.revisionId,
        originalRevisionUnchanged: true,
        oldRevisionResultCount:
          Object.keys(oldAfterRepair.query_results ?? {}).length,
        repairedSchemaFingerprint: repairedQuery!.query!.schema_fingerprint,
        originalSchemaFingerprint: originalQuery!.query!.schema_fingerprint,
        currentSchemaFingerprint: currentSchemaQuery!.schema_fingerprint,
        projectedColumns: repairedQuery!.returns!.columns!.map((column) =>
          column.name
        ),
        repairedEvaluationColumns: repairedRows!.columns!.map((column) =>
          column.name
        ),
        repairedEvaluationIncludesNewField: JSON.stringify(repairedRows?.rows)
          .includes("current Form descriptor"),
        renderedTableHeaders,
      }),
    );
  } finally {
    await ownerContext.close();
  }
});
