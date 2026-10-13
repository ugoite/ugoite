import {
  type APIRequestContext,
  type Browser,
  type BrowserContext,
  expect,
  test,
} from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";
import { openIsolatedPasskeyPage } from "./lib/security-context.ts";

const SOURCE_UNAVAILABLE_DIAGNOSTIC = "A data source is unavailable.";
const COMPOSITION_SOURCE_ID = "shared_query_source";
const INPUT_COMPONENT_ID = "shared_query_input";
const METRIC_COMPONENT_ID = "shared_result_metric";
const TABLE_COMPONENT_ID = "shared_result_table";
const VARIABLE_ID = "query_key";
const SQL_VARIABLE_NAME = "query_key";
const METRIC_QUERY_PATH = "/compositions/metric-query";

type CompositionSeed = {
  spaceId: string;
  formId: string;
  formName: string;
  savedSqlId: string;
  savedSqlRevisionId: string;
  savedSqlName: string;
  savedSqlText: string;
  compositionId: string;
  compositionRevisionId: string;
  compositionName: string;
  protectedValue: string;
};

type MetricResponse = {
  httpStatus?: number;
  ok?: boolean;
  result?: { component_id: string; value: unknown };
  diagnostics?: Array<{ code: string }>;
  code?: string;
  message?: string;
};

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
    data: {
      slug: `composition-isolation-${label}-${suffix}`,
      name,
    },
  });
  expect([200, 201]).toContain(response.status());
  const spaceId = (await response.json() as { space_uid?: string }).space_uid;
  expect(spaceId).toBeTruthy();
  return { id: spaceId!, name };
}

async function seedSavedSqlMetric(
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

  const savedSqlName = `Source ${options.compositionName}`;
  const sql = `SELECT ${sqlIdentifier(protectedColumn!)} AS value FROM ${
    sqlIdentifier(relation!)
  } WHERE ${sqlIdentifier(lookupColumn!)} = $${SQL_VARIABLE_NAME}`;
  const sqlResponse = await request.post(
    getBackendUrl(`/spaces/${options.spaceId}/sql`),
    {
      data: {
        name: savedSqlName,
        kind: "user-query",
        sql,
        variables: [{
          type: "string",
          name: SQL_VARIABLE_NAME,
          description: "Select the matching row.",
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

  const yaml = [
    "format: ugoite.composition",
    "format_version: 1",
    `name: ${options.compositionName}`,
    "kind: dashboard",
    "tags: []",
    "spec:",
    "  variables:",
    `    - id: ${VARIABLE_ID}`,
    "      type: string",
    "      required: true",
    "      default: match",
    "  sources:",
    `    - id: ${COMPOSITION_SOURCE_ID}`,
    "      kind: saved_sql",
    `      entry_id: "${savedSql.id}"`,
    `      revision_id: "${savedSql.revision_id}"`,
    "      expected_result:",
    "        - name: value",
    "          type: string",
    "      variables:",
    `        ${SQL_VARIABLE_NAME}:`,
    `          variable: ${VARIABLE_ID}`,
    "  components:",
    `    - id: ${INPUT_COMPONENT_ID}`,
    "      kind: input",
    "      label: Lookup key",
    `      variable: ${VARIABLE_ID}`,
    `    - id: ${METRIC_COMPONENT_ID}`,
    "      kind: metric",
    "      label: Protected result",
    `      source: ${COMPOSITION_SOURCE_ID}`,
    "      column: value",
    `    - id: ${TABLE_COMPONENT_ID}`,
    "      kind: table",
    `      source: ${COMPOSITION_SOURCE_ID}`,
    "  layout:",
    "    kind: flow",
    "    rows:",
    "      - id: primary",
    "        items:",
    "          - kind: component",
    `            component: ${INPUT_COMPONENT_ID}`,
    "          - kind: component",
    `            component: ${METRIC_COMPONENT_ID}`,
    "          - kind: component",
    `            component: ${TABLE_COMPONENT_ID}`,
    "",
  ].join("\n");
  const compositionResponse = await request.post(
    getBackendUrl(`/spaces/${options.spaceId}/compositions`),
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
    spaceId: options.spaceId,
    formId: form!.id!,
    formName: options.formName,
    savedSqlId: savedSql.id!,
    savedSqlRevisionId: savedSql.revision_id!,
    savedSqlName,
    savedSqlText: sql,
    compositionId: composition.composition_id!,
    compositionRevisionId: composition.revision_id!,
    compositionName: options.compositionName,
    protectedValue: options.protectedValue,
  };
}

async function metricResponse(
  page: import("@playwright/test").Page,
): Promise<MetricResponse> {
  const response = await page.waitForResponse((candidate) =>
    candidate.url().includes(METRIC_QUERY_PATH)
  );
  return {
    ...await response.json() as Omit<MetricResponse, "httpStatus">,
    httpStatus: response.status(),
  };
}

function sqlPageResponse(page: import("@playwright/test").Page) {
  return page.waitForResponse((candidate) =>
    new URL(candidate.url()).pathname.endsWith("/sql/query")
  );
}

function compositionPath(seed: CompositionSeed): string {
  return `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/${seed.compositionRevisionId}`;
}

test.describe("Composition Saved SQL ACL and runtime isolation", () => {
  test.beforeAll(async ({ request }) => await waitForServers(request));

  test("Viewer Metric and Table clear after Form or Saved SQL Read is revoked", async ({ browser, context, request }) => {
    test.setTimeout(180_000);
    const space = await createSpace(request, "acl-revocation");
    const suffix = crypto.randomUUID().slice(0, 8);
    const seed = await seedSavedSqlMetric(request, {
      spaceId: space.id,
      formName: `AclMetricSource${suffix}`,
      compositionName: `Revoked source ${suffix}`,
      protectedValue: `protected-sql-value-${suffix}`,
    });

    const invitation = await request.post(
      getBackendUrl(`/spaces/${seed.spaceId}/members/invitations`),
      { data: { label: `ACL viewer ${suffix}`, role: "viewer" } },
    );
    expect(invitation.status()).toBe(201);
    const invitationUrl =
      (await invitation.json() as { invitation_url?: string }).invitation_url;
    expect(invitationUrl).toBeTruthy();

    const { target: viewerContext, page, close } =
      await openIsolatedPasskeyPage(browser);
    try {
      await page.goto(invitationUrl!);
      await page.getByRole("button", { name: "Accept invitation" }).click();
      await expect(page).toHaveURL(/\/spaces$/);

      const membersResponse = await request.get(
        getBackendUrl(`/spaces/${seed.spaceId}/members`),
      );
      expect(membersResponse.ok()).toBe(true);
      const members = await membersResponse.json() as Array<{
        principal: { principal_id: string; display_name: string };
        role: string;
      }>;
      const viewer = members.find((member) =>
        member.principal.display_name === `ACL viewer ${suffix}`
      );
      expect(viewer?.role).toBe("viewer");
      expect(viewer?.principal.principal_id).toBeTruthy();

      const initialMetricResponse = metricResponse(page);
      const initialSqlPageResponse = sqlPageResponse(page);
      await page.goto(getFrontendUrl(compositionPath(seed)), {
        waitUntil: "domcontentloaded",
      });
      await expect(
        page.getByRole("heading", { level: 1, name: seed.compositionName }),
      ).toBeVisible();
      await expect(page.locator("output.compositionMetric")).toContainText(
        seed.protectedValue,
      );
      await expect(page.getByRole("table")).toContainText(seed.protectedValue);
      const initialBody = await initialMetricResponse;
      const initialSqlResponse = await initialSqlPageResponse;
      expect(initialBody.ok).toBe(true);
      expect(initialBody.result).toMatchObject({
        component_id: METRIC_COMPONENT_ID,
        value: seed.protectedValue,
      });
      expect(initialSqlResponse.ok()).toBe(true);
      const authorizedSqlRequest = initialSqlResponse.request().postDataJSON();

      const compositionRead = await viewerContext.request.get(
        getBackendUrl(
          `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/history/${seed.compositionRevisionId}`,
        ),
      );
      expect(compositionRead.ok()).toBe(true);

      const assertSourceReadRevocation = async (
        inputValue: string,
        revokedRead: "form" | "saved_sql",
      ) => {
        // Replay the exact request captured while authorized. SQL execution
        // must re-check current source access rather than trust an earlier plan.
        const replayedSqlResponse = await viewerContext.request.post(
          getBackendUrl(`/spaces/${seed.spaceId}/sql/query`),
          { data: authorizedSqlRequest },
        );
        const replayedSqlBody = await replayedSqlResponse.text();
        expect(replayedSqlResponse.ok()).toBe(false);

        const compositionReadAfterRevocation = await viewerContext.request.get(
          getBackendUrl(
            `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/history/${seed.compositionRevisionId}`,
          ),
        );
        expect(compositionReadAfterRevocation.ok()).toBe(true);

        const revokedMetricResponse = metricResponse(page);
        await page.getByRole("textbox", { name: "Lookup key", exact: true })
          .fill(inputValue);
        await expect(page.locator("output.compositionMetric")).toHaveCount(0);
        const deniedBody = await revokedMetricResponse;
        await expect(page.getByRole("table")).toHaveCount(0);
        await expect(page.locator("body")).not.toContainText(
          seed.protectedValue,
        );
        if (revokedRead === "saved_sql") {
          expect(deniedBody).toMatchObject({
            ok: false,
            diagnostics: [{ code: "source_unavailable" }],
          });
        } else {
          expect(deniedBody.httpStatus).toBeGreaterThanOrEqual(400);
        }

        // Revisit the same revision. A revoked Saved SQL source is hidden
        // during resolution; a still-readable Saved SQL source can resolve,
        // but its execution remains blocked by the revoked Form Read policy.
        const reloadedSqlResponse = revokedRead === "form"
          ? sqlPageResponse(page)
          : undefined;
        await page.reload({ waitUntil: "domcontentloaded" });
        if (reloadedSqlResponse) {
          const response = await reloadedSqlResponse;
          expect(response.ok()).toBe(false);
        }
        const resolveResponse = await viewerContext.request.post(
          getBackendUrl(
            `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/resolve`,
          ),
          {
            data: {
              revision_id: seed.compositionRevisionId,
              variables: { [VARIABLE_ID]: "match" },
            },
          },
        );
        expect(resolveResponse.ok()).toBe(true);
        const resolveBody = await resolveResponse.json() as {
          ok: boolean;
          diagnostics?: Array<{ code: string }>;
        };
        if (revokedRead === "saved_sql") {
          expect(resolveBody).toMatchObject({
            ok: false,
            diagnostics: [{ code: "source_unavailable" }],
          });
        } else {
          expect(resolveBody.ok).toBe(true);
        }
        const deniedText = JSON.stringify([deniedBody, resolveBody]);
        const concealedValues = [
          seed.savedSqlText,
          seed.protectedValue,
          ...(revokedRead === "form" ? [seed.formId, seed.formName] : []),
          ...(revokedRead === "saved_sql"
            ? [seed.savedSqlId, seed.savedSqlRevisionId, seed.savedSqlName]
            : []),
        ];
        for (const privateValue of concealedValues) {
          expect(deniedText).not.toContain(privateValue);
          expect(replayedSqlBody).not.toContain(privateValue);
          await expect(page.locator("body")).not.toContainText(privateValue);
        }
        if (revokedRead === "saved_sql") {
          await expect(
            page.getByRole("alert").filter({
              hasText: SOURCE_UNAVAILABLE_DIAGNOSTIC,
            }),
          ).toBeVisible();
        }
        await expect(page.locator("output.compositionMetric")).toHaveCount(0);
        await expect(page.getByRole("table")).toHaveCount(0);
        await expect(page.locator("body")).not.toContainText(
          seed.protectedValue,
        );
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
      await assertSourceReadRevocation("form-access-revoked", "form");

      const restoreFormPolicyResponse = await request.put(
        getBackendUrl(`/spaces/${seed.spaceId}/policies/form/${seed.formName}`),
        {
          data: {
            policy_id: crypto.randomUUID(),
            inherit_space_role: true,
            grants: [],
          },
        },
      );
      expect(restoreFormPolicyResponse.status()).toBe(200);
      const restoredMetricResponse = metricResponse(page);
      const restoredSqlPageResponse = sqlPageResponse(page);
      await page.reload({ waitUntil: "domcontentloaded" });
      await expect(page.locator("output.compositionMetric")).toContainText(
        seed.protectedValue,
      );
      await expect(page.getByRole("table")).toContainText(seed.protectedValue);
      expect((await restoredMetricResponse).result?.value).toBe(
        seed.protectedValue,
      );
      expect((await restoredSqlPageResponse).ok()).toBe(true);

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
      await assertSourceReadRevocation("saved-sql-access-revoked", "saved_sql");

      const ownerContext = await browser.newContext({
        storageState: await context.storageState(),
      });
      try {
        const ownerPage = await ownerContext.newPage();
        const ownerMetricResponse = metricResponse(ownerPage);
        await ownerPage.goto(getFrontendUrl(compositionPath(seed)), {
          waitUntil: "domcontentloaded",
        });
        await expect(ownerPage.locator("output.compositionMetric"))
          .toContainText(seed.protectedValue);
        await expect(ownerPage.getByRole("table")).toContainText(
          seed.protectedValue,
        );
        expect((await ownerMetricResponse).result?.value).toBe(
          seed.protectedValue,
        );
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

  test("Space switching shows only the selected Metric result", async ({ browser, context, request }) => {
    test.setTimeout(180_000);
    const spaceA = await createSpace(request, "isolation-a");
    const spaceB = await createSpace(request, "isolation-b");
    const compositionName = "Shared identifiers";
    const formName = "IsolationSource";
    const seedA = await seedSavedSqlMetric(request, {
      spaceId: spaceA.id,
      compositionName,
      formName,
      protectedValue: "space-a-protected-value",
    });
    const seedB = await seedSavedSqlMetric(request, {
      spaceId: spaceB.id,
      compositionName,
      formName,
      protectedValue: "space-b-protected-value",
    });
    expect(seedA.formName).toBe(seedB.formName);

    const ownerState = await context.storageState();
    const ownerContext: BrowserContext = await browser.newContext({
      storageState: ownerState,
    });
    try {
      const page = await ownerContext.newPage();
      const firstMetricResponse = metricResponse(page);
      const firstSqlPageResponse = sqlPageResponse(page);
      await page.goto(getFrontendUrl(compositionPath(seedA)), {
        waitUntil: "domcontentloaded",
      });
      await expect(page.locator("output.compositionMetric")).toContainText(
        seedA.protectedValue,
      );
      await expect(page.getByRole("table")).toContainText(seedA.protectedValue);
      await expect(
        page.getByRole("textbox", { name: "Lookup key", exact: true }),
      ).toHaveValue("match");
      expect((await firstMetricResponse).result?.value).toBe(
        seedA.protectedValue,
      );
      expect((await firstSqlPageResponse).ok()).toBe(true);

      const spaceSelector = page.getByRole("combobox", { name: "Space" });
      await expect(spaceSelector.getByRole("option", { name: spaceB.name }))
        .toHaveCount(1);
      await spaceSelector.selectOption(seedB.spaceId);
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${seedB.spaceId}/compositions$`),
      );
      const secondMetricResponse = metricResponse(page);
      const secondSqlPageResponse = sqlPageResponse(page);
      await page.getByRole("link", { name: new RegExp(`^${compositionName}`) })
        .click();
      await expect(page.locator("output.compositionMetric")).toContainText(
        seedB.protectedValue,
      );
      await expect(page.getByRole("table")).toContainText(seedB.protectedValue);
      await expect(
        page.getByRole("textbox", { name: "Lookup key", exact: true }),
      ).toHaveValue("match");
      expect((await secondMetricResponse).result?.value).toBe(
        seedB.protectedValue,
      );
      expect((await secondSqlPageResponse).ok()).toBe(true);
      await expect(page.locator("body")).not.toContainText(
        seedA.protectedValue,
      );

      await page.getByRole("combobox", { name: "Space" })
        .selectOption(seedA.spaceId);
      await expect(page).toHaveURL(
        new RegExp(`/spaces/${seedA.spaceId}/compositions$`),
      );
      const thirdMetricResponse = metricResponse(page);
      const thirdSqlPageResponse = sqlPageResponse(page);
      await page.getByRole("link", { name: new RegExp(`^${compositionName}`) })
        .click();
      await expect(page.locator("output.compositionMetric")).toContainText(
        seedA.protectedValue,
      );
      await expect(page.getByRole("table")).toContainText(seedA.protectedValue);
      expect((await thirdMetricResponse).result?.value).toBe(
        seedA.protectedValue,
      );
      expect((await thirdSqlPageResponse).ok()).toBe(true);
      await expect(page.locator("body")).not.toContainText(
        seedB.protectedValue,
      );
    } finally {
      await ownerContext.close();
    }
  });
});
