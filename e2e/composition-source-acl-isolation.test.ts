import {
  type APIRequestContext,
  type Browser,
  type BrowserContext,
  expect,
  test,
} from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";
import { completeCompositionGraphYaml } from "./lib/composition-graph.ts";
import { openIsolatedPasskeyPage } from "./lib/security-context.ts";

const VARIABLE_ID = "query_key";
const METRIC_QUERY_ID = "shared_metric";
const TABLE_QUERY_ID = "shared_rows";

type CompositionSeed = {
  spaceId: string;
  formId: string;
  formName: string;
  savedSqlId: string;
  savedSqlRevisionId: string;
  savedSqlName: string;
  compositionId: string;
  compositionRevisionId: string;
  compositionName: string;
  protectedValue: string;
};

type EvaluationBody = {
  ok: boolean;
  query_results?: Record<string, unknown>;
  diagnostics?: Array<{ code?: string }>;
  query_diagnostics?: Array<{ variable_id: string; code?: string }>;
};

type SavedComposition = { composition_id?: string; revision_id?: string };

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

async function createSpace(
  request: APIRequestContext,
  label: string,
): Promise<{ id: string; name: string }> {
  const suffix = `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
  const name = `Composition isolation ${label} ${suffix}`;
  const response = await request.post(getBackendUrl("/spaces"), {
    data: { slug: `composition-isolation-${label}-${suffix}`, name },
  });
  expect([200, 201]).toContain(response.status());
  const spaceId = (await response.json() as { space_uid?: string }).space_uid;
  expect(spaceId).toBeTruthy();
  return { id: spaceId!, name };
}

async function seedGraph(
  request: APIRequestContext,
  options: {
    spaceId: string;
    compositionName: string;
    formName: string;
    protectedValue: string;
  },
): Promise<CompositionSeed> {
  const formResponse = await request.post(
    getBackendUrl(`/spaces/${options.spaceId}/forms`),
    {
      data: {
        name: options.formName,
        version: 1,
        template:
          `# ${options.formName}\n\n## Lookup key\n\n## Protected value\n`,
        fields: {
          "Lookup key": { type: "string", required: true },
          "Protected value": { type: "string", required: true },
        },
      },
    },
  );
  expect([200, 201]).toContain(formResponse.status());

  const formsResponse = await request.get(
    getBackendUrl(`/spaces/${options.spaceId}/forms`),
  );
  expect(formsResponse.ok()).toBe(true);
  const forms = await formsResponse.json() as Array<{
    id?: string;
    name?: string;
    sql_relation?: string;
    fields?: Record<string, { sql_column?: string }>;
  }>;
  const form = forms.find((candidate) => candidate.name === options.formName);
  const relation = form?.sql_relation;
  const lookupColumn = form?.fields?.["Lookup key"]?.sql_column;
  const protectedColumn = form?.fields?.["Protected value"]?.sql_column;
  expect(form?.id).toBeTruthy();
  expect(relation).toBeTruthy();
  expect(lookupColumn).toBeTruthy();
  expect(protectedColumn).toBeTruthy();

  const entryResponse = await request.post(
    getBackendUrl(`/spaces/${options.spaceId}/entries`),
    {
      data: {
        form: options.formName,
        fields: {
          "Lookup key": "match",
          "Protected value": options.protectedValue,
        },
      },
    },
  );
  expect(entryResponse.status()).toBe(201);

  const savedSqlName = `Protected lookup ${options.compositionName}`;
  const sql = `SELECT ${sqlIdentifier(protectedColumn!)} AS value FROM ${
    sqlIdentifier(relation!)
  } WHERE ${sqlIdentifier(lookupColumn!)} = $query_key`;
  const savedSqlResponse = await request.post(
    getBackendUrl(`/spaces/${options.spaceId}/sql`),
    {
      data: {
        name: savedSqlName,
        kind: "user-query",
        sql,
        variables: [{ name: "query_key", type: "string", description: "" }],
      },
    },
  );
  expect([200, 201]).toContain(savedSqlResponse.status());
  const savedSql = await savedSqlResponse.json() as {
    id?: string;
    revision_id?: string;
  };
  expect(savedSql.id).toBeTruthy();
  expect(savedSql.revision_id).toBeTruthy();

  const argumentsForSavedSql = [{ name: "query_key", variable: VARIABLE_ID }];
  const yaml = await completeCompositionGraphYaml(request, options.spaceId, {
    name: options.compositionName,
    variables: [
      {
        id: VARIABLE_ID,
        kind: "input",
        value_type: { scalar_type: "string", nullable: false },
        required: true,
        default: { kind: "string", value: "match" },
      },
      {
        id: METRIC_QUERY_ID,
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "string", nullable: true },
        },
        query: {
          kind: "saved_sql",
          entry_id: savedSql.id!,
          revision_id: savedSql.revision_id!,
          column: "value",
          arguments: argumentsForSavedSql,
        },
      },
      {
        id: TABLE_QUERY_ID,
        kind: "query",
        returns: { kind: "table", columns: [] },
        query: {
          kind: "saved_sql",
          entry_id: savedSql.id!,
          revision_id: savedSql.revision_id!,
          arguments: argumentsForSavedSql,
        },
      },
    ],
    components: [
      { id: "query_input", kind: "input", variable: VARIABLE_ID },
      { id: "result_metric", kind: "metric", variable: METRIC_QUERY_ID },
      { id: "result_table", kind: "table", variable: TABLE_QUERY_ID },
    ],
    rows: [{
      id: "primary",
      items: [
        { kind: "component", component: "query_input" },
        { kind: "component", component: "result_metric" },
        { kind: "component", component: "result_table" },
      ],
    }],
  });
  const compositionResponse = await request.post(
    getBackendUrl(`/spaces/${options.spaceId}/compositions`),
    {
      headers: { "Idempotency-Key": crypto.randomUUID() },
      data: { yaml },
    },
  );
  expect([200, 201]).toContain(compositionResponse.status());
  const composition = await compositionResponse.json() as SavedComposition;
  expect(composition.composition_id).toBeTruthy();
  expect(composition.revision_id).toBeTruthy();

  return {
    spaceId: options.spaceId,
    formId: form!.id!,
    formName: options.formName,
    savedSqlId: savedSql.id!,
    savedSqlRevisionId: savedSql.revision_id!,
    savedSqlName,
    compositionId: composition.composition_id!,
    compositionRevisionId: composition.revision_id!,
    compositionName: options.compositionName,
    protectedValue: options.protectedValue,
  };
}

function evaluationResponse(page: import("@playwright/test").Page) {
  return page.waitForResponse((response) =>
    new URL(response.url()).pathname.endsWith("/compositions/evaluate")
  );
}

function compositionPath(seed: CompositionSeed): string {
  return `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/${seed.compositionRevisionId}`;
}

const evaluateExactRevision = (
  request: APIRequestContext,
  seed: CompositionSeed,
) =>
  request.post(
    getBackendUrl(`/spaces/${seed.spaceId}/compositions/evaluate`),
    {
      data: {
        evaluation_id: crypto.randomUUID(),
        target: {
          kind: "saved_revision",
          composition_id: seed.compositionId,
          revision_id: seed.compositionRevisionId,
        },
        variables: { [VARIABLE_ID]: "match" },
        requested_queries: [METRIC_QUERY_ID, TABLE_QUERY_ID],
      },
    },
  );

test.describe("Composition graph ACL and runtime isolation", () => {
  test.beforeAll(async ({ request }) => await waitForServers(request));

  test("evaluate rechecks Form and exact Saved SQL access after revocation", async ({ browser, context, request }) => {
    test.setTimeout(180_000);
    const space = await createSpace(request, "acl-revocation");
    const suffix = crypto.randomUUID().slice(0, 8);
    const seed = await seedGraph(request, {
      spaceId: space.id,
      formName: `AclForm${suffix}`,
      compositionName: `Revoked graph ${suffix}`,
      protectedValue: `protected-value-${suffix}`,
    });

    const invitation = await request.post(
      getBackendUrl(`/spaces/${seed.spaceId}/members/invitations`),
      { data: { label: `ACL viewer ${suffix}`, role: "viewer" } },
    );
    expect(invitation.status()).toBe(201);
    const invitationUrl =
      (await invitation.json() as { invitation_url?: string })
        .invitation_url;
    expect(invitationUrl).toBeTruthy();

    const { target: viewerContext, page, close } =
      await openIsolatedPasskeyPage(browser);
    try {
      await page.goto(invitationUrl!);
      await page.getByRole("button", { name: "Accept invitation" }).click();
      await expect(page).toHaveURL(/\/spaces$/);

      const initialEvaluation = evaluationResponse(page);
      await page.goto(getFrontendUrl(compositionPath(seed)), {
        waitUntil: "domcontentloaded",
      });
      await expect(
        page.getByRole("heading", { level: 1, name: seed.compositionName }),
      )
        .toBeVisible();
      await expect(page.locator(".compositionGraphMetric strong"))
        .toContainText(seed.protectedValue);
      await expect(page.getByRole("table")).toContainText(seed.protectedValue);
      const authorized = await initialEvaluation;
      expect(authorized.ok()).toBe(true);
      const authorizedBody = await authorized.json() as EvaluationBody;
      expect(authorizedBody.ok).toBe(true);
      expect(JSON.stringify(authorizedBody)).toContain(seed.protectedValue);

      const compositionRead = await viewerContext.request.get(
        getBackendUrl(
          `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/history/${seed.compositionRevisionId}`,
        ),
      );
      expect(compositionRead.ok()).toBe(true);

      const assertRevokedEvaluation = async (
        policyKind: "form" | "saved_sql",
        resourceId: string,
      ) => {
        const denied = await evaluateExactRevision(viewerContext.request, seed);
        const bodyText = await denied.text();
        expect([200, 403]).toContain(denied.status());
        expect(bodyText).not.toContain(seed.protectedValue);
        if (denied.ok()) {
          const body = JSON.parse(bodyText) as EvaluationBody;
          expect(body.ok).toBe(false);
          expect(body.query_results ?? {}).toEqual({});
        }
        expect(bodyText).not.toContain(resourceId);

        const browserEvaluation = evaluationResponse(page);
        await page.getByRole("textbox", { name: VARIABLE_ID }).fill("match-2");
        const browserResponse = await browserEvaluation;
        expect([200, 403]).toContain(browserResponse.status());
        const browserBody = await browserResponse.text();
        expect(browserBody).not.toContain(seed.protectedValue);
        await expect(page.locator("body")).not.toContainText(
          seed.protectedValue,
        );
        await expect(page.locator(".compositionGraphMetric")).toHaveCount(0);
        await expect(page.getByRole("table")).not.toContainText(
          seed.protectedValue,
        );

        // Raw exact revision inspection remains available to the authorized
        // Space member even when its query dependencies are no longer readable.
        const raw = await viewerContext.request.get(getBackendUrl(
          `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/history/${seed.compositionRevisionId}`,
        ));
        expect(raw.ok()).toBe(true);
        const rawBody = await raw.text();
        expect(rawBody).toContain(seed.compositionName);
        expect(rawBody).toContain(policyKind);
      };

      const formPolicyResponse = await request.put(
        getBackendUrl(`/spaces/${seed.spaceId}/policies/form/${seed.formName}`),
        {
          data: {
            policy_id: crypto.randomUUID(),
            inherit_space_role: false,
            grants: [],
          },
        },
      );
      expect(formPolicyResponse.status()).toBe(200);
      await assertRevokedEvaluation("form", seed.formId);

      const restoreFormPolicy = await request.put(
        getBackendUrl(`/spaces/${seed.spaceId}/policies/form/${seed.formName}`),
        {
          data: {
            policy_id: crypto.randomUUID(),
            inherit_space_role: true,
            grants: [],
          },
        },
      );
      expect(restoreFormPolicy.status()).toBe(200);
      const restoredEvaluation = evaluationResponse(page);
      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(page.locator(".compositionGraphMetric strong"))
        .toContainText(seed.protectedValue);
      expect((await restoredEvaluation).ok()).toBe(true);

      const savedSqlPolicyResponse = await request.put(
        getBackendUrl(
          `/spaces/${seed.spaceId}/policies/saved_sql/${seed.savedSqlId}`,
        ),
        {
          data: {
            policy_id: crypto.randomUUID(),
            inherit_space_role: false,
            grants: [],
          },
        },
      );
      expect(savedSqlPolicyResponse.status()).toBe(200);
      await assertRevokedEvaluation("saved_sql", seed.savedSqlId);

      const ownerContext = await browser.newContext({
        storageState: await context.storageState(),
      });
      try {
        const ownerPage = await ownerContext.newPage();
        const ownerEvaluation = evaluationResponse(ownerPage);
        await ownerPage.goto(getFrontendUrl(compositionPath(seed)), {
          waitUntil: "domcontentloaded",
        });
        await expect(ownerPage.locator(".compositionGraphMetric strong"))
          .toContainText(seed.protectedValue);
        expect((await ownerEvaluation).ok()).toBe(true);
        await expect(page.locator("body")).not.toContainText(
          seed.protectedValue,
        );
      } finally {
        await ownerContext.close();
      }
    } finally {
      await close();
    }
  });

  test("switching Spaces shows only the selected graph evaluation", async ({ browser, context, request }) => {
    test.setTimeout(180_000);
    const spaceA = await createSpace(request, "isolation-a");
    const spaceB = await createSpace(request, "isolation-b");
    const compositionName = "Shared graph identifiers";
    const formName = "IsolationForm";
    const seedA = await seedGraph(request, {
      spaceId: spaceA.id,
      compositionName,
      formName,
      protectedValue: "space-a-private-value",
    });
    const seedB = await seedGraph(request, {
      spaceId: spaceB.id,
      compositionName,
      formName,
      protectedValue: "space-b-private-value",
    });
    expect(seedA.formName).toBe(seedB.formName);

    const ownerContext: BrowserContext = await browser.newContext({
      storageState: await context.storageState(),
    });
    try {
      const page = await ownerContext.newPage();
      const firstEvaluation = evaluationResponse(page);
      await page.goto(getFrontendUrl(compositionPath(seedA)), {
        waitUntil: "domcontentloaded",
      });
      await expect(page.locator(".compositionGraphMetric strong"))
        .toContainText(seedA.protectedValue);
      await expect(page.getByRole("table")).toContainText(seedA.protectedValue);
      await expect(page.getByRole("textbox", { name: VARIABLE_ID }))
        .toHaveValue("match");
      expect((await firstEvaluation).ok()).toBe(true);

      const spaceSelector = page.getByRole("combobox", { name: "Space" });
      await spaceSelector.selectOption(seedB.spaceId);
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${seedB.spaceId}/compositions$`),
      );
      const secondEvaluation = evaluationResponse(page);
      await page.getByRole("link", { name: new RegExp(`^${compositionName}`) })
        .click();
      await expect(page.locator(".compositionGraphMetric strong"))
        .toContainText(seedB.protectedValue);
      await expect(page.getByRole("table")).toContainText(seedB.protectedValue);
      expect((await secondEvaluation).ok()).toBe(true);
      await expect(page.locator("body")).not.toContainText(
        seedA.protectedValue,
      );
    } finally {
      await ownerContext.close();
    }
  });
});
