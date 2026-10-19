import { type APIRequestContext, expect, test } from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";
import { completeSavedSqlCompositionYaml } from "./lib/composition.ts";

const SOURCE_UNAVAILABLE_DIAGNOSTIC = "A data source is unavailable.";

type SeedState = {
  spaceId: string;
  compositionId: string;
  revisionId: string;
  compositionName: string;
  canonicalYaml: string;
};

type CompositionSaveResponse = {
  composition_id?: string;
  revision_id?: string;
  canonical_yaml?: string;
};

type CompositionRawRevision = {
  revision: { entry_id: string; revision_id: string };
  fields: Record<string, unknown>;
  unmapped_field_values: Record<string, unknown>;
};

type CompositionHistoryPage = {
  total?: number;
  revisions?: CompositionRawRevision[];
};

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

async function createSpace(request: APIRequestContext): Promise<string> {
  const suffix = `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
  const response = await request.post(getBackendUrl("/spaces"), {
    data: {
      slug: `composition-recovery-${suffix}`,
      name: `Composition Recovery ${suffix}`,
    },
  });
  expect([200, 201]).toContain(response.status());
  const result = await response.json() as { space_uid?: string };
  expect(result.space_uid).toBeTruthy();
  return result.space_uid!;
}

async function seedUnavailableSourceComposition(
  request: APIRequestContext,
): Promise<SeedState> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const compositionName = `Broken recovery ${suffix}`;
  const formName = `RecoverySource${suffix}`;
  const formResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/forms`),
    {
      data: {
        name: formName,
        version: 1,
        template: `# ${formName}\n\n## Private value\n`,
        fields: { "Private value": { type: "string", required: true } },
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
  const valueColumn = form?.fields?.["Private value"]?.sql_column;
  expect(relation).toBeTruthy();
  expect(valueColumn).toBeTruthy();

  const entryResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/entries`),
    {
      data: {
        form: formName,
        fields: { "Private value": `recovery-private-${suffix}` },
      },
    },
  );
  expect(entryResponse.status()).toBe(201);

  const savedSqlName = `Recovery source ${suffix}`;
  const savedSqlText = `SELECT ${sqlIdentifier(valueColumn!)} AS value FROM ${
    sqlIdentifier(relation!)
  }`;
  const savedSqlResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name: savedSqlName,
        kind: "user-query",
        sql: savedSqlText,
        variables: [],
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

  let yaml = [
    "format: ugoite.composition",
    "format_version: 1",
    `name: ${compositionName}`,
    "kind: dashboard",
    "spec:",
    "  sources:",
    "    - id: recovery_rows",
    "      kind: saved_sql",
    `      entry_id: "${savedSql.id}"`,
    `      revision_id: "${savedSql.revision_id}"`,
    "      expected_result: []",
    "      schema_fingerprint: ''",
    "  components:",
    "    - id: recovery_table",
    "      kind: table",
    "      source: recovery_rows",
    "  layout:",
    "    kind: flow",
    "    rows:",
    "      - id: main",
    "        items:",
    "          - kind: component",
    "            component: recovery_table",
    "",
  ].join("\n");
  yaml = await completeSavedSqlCompositionYaml(request, spaceId, yaml, [{
    id: "recovery_rows",
    savedSqlId: savedSql.id!,
    savedSqlRevisionId: savedSql.revision_id!,
  }]);
  const saveResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions`),
    {
      headers: { "Idempotency-Key": crypto.randomUUID() },
      data: { yaml },
    },
  );
  expect([200, 201]).toContain(saveResponse.status());
  const saved = await saveResponse.json() as CompositionSaveResponse;
  expect(saved.composition_id).toBeTruthy();
  expect(saved.revision_id).toBeTruthy();
  expect(saved.canonical_yaml).toBeTruthy();

  return {
    spaceId,
    compositionId: saved.composition_id!,
    revisionId: saved.revision_id!,
    compositionName,
    canonicalYaml: saved.canonical_yaml!,
  };
}

test.describe("Composition Recovery Authorization", () => {
  let seed: SeedState;

  test.beforeAll(async ({ request }) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    seed = await seedUnavailableSourceComposition(request);
  });

  test("Composition raw recovery preserves caller-visible contracts", async ({ browser, context, playwright, request }) => {
    test.setTimeout(120_000);

    // Typed inspect (latest), the exact raw revision, and history preserve
    // the stored spec byte-identically.
    const latestResponse = await request.get(
      getBackendUrl(
        `/spaces/${seed.spaceId}/compositions/${seed.compositionId}`,
      ),
    );
    expect(latestResponse.ok()).toBe(true);
    const latest = await latestResponse.json() as CompositionRawRevision;
    expect(latest.revision.entry_id).toBe(seed.compositionId);
    expect(latest.revision.revision_id).toBe(seed.revisionId);
    expect(latest.fields["name"]).toBe(seed.compositionName);
    expect(latest.fields["spec"]).toBe(seed.canonicalYaml);

    const exactResponse = await request.get(
      getBackendUrl(
        `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/history/${seed.revisionId}`,
      ),
    );
    expect(exactResponse.ok()).toBe(true);
    const exact = await exactResponse.json() as CompositionRawRevision;
    expect(exact.revision.revision_id).toBe(seed.revisionId);
    expect(exact.fields["spec"]).toBe(seed.canonicalYaml);

    const historyResponse = await request.get(
      getBackendUrl(
        `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/history?limit=100&offset=0`,
      ),
    );
    expect(historyResponse.ok()).toBe(true);
    const history = await historyResponse.json() as CompositionHistoryPage;
    expect(history.total).toBe(1);
    expect(history.revisions).toHaveLength(1);
    expect(history.revisions![0]!.fields["spec"]).toBe(seed.canonicalYaml);

    // A missing composition read returns the generic error shape.
    // Authenticated-denied reads share this shape by construction; that
    // boundary is covered by Rust storage/ACL tests, not by a second
    // principal here: generic_entry_mutations_cannot_write_composition_registry
    // plus the concealment unit tests in
    // crates/ugoite-core/src/composition.rs
    // (denied_metric_source_is_concealed_as_source_unavailable and
    // field_level_diagnostics_require_an_authorized_source_form_read).
    const missingResponse = await request.get(
      getBackendUrl(
        `/spaces/${seed.spaceId}/compositions/${crypto.randomUUID()}`,
      ),
    );
    expect(missingResponse.status()).toBe(404);
    const missing = await missingResponse.json() as {
      code?: unknown;
      message?: unknown;
    };
    expect(missing.code).toBe("ENTRY_NOT_FOUND");
    expect(typeof missing.message).toBe("string");
    expect(JSON.stringify(missing)).not.toContain(seed.compositionName);

    // An unauthenticated read returns the generic auth shape. The context
    // carries an explicitly empty storage state: a bare newContext would
    // inherit the shared owner session from the Playwright project config.
    const anonymous = await playwright.request.newContext({
      storageState: { cookies: [], origins: [] },
      extraHTTPHeaders: { Origin: new URL(getFrontendUrl("/")).origin },
    });
    try {
      const deniedResponse = await anonymous.get(
        getBackendUrl(
          `/spaces/${seed.spaceId}/compositions/${seed.compositionId}`,
        ),
      );
      expect(deniedResponse.status()).toBe(401);
      const denied = await deniedResponse.json() as {
        code?: unknown;
        message?: unknown;
      };
      expect(denied.code).toBe("AUTHENTICATION_REQUIRED");
      expect(typeof denied.message).toBe("string");
    } finally {
      await anonymous.dispose();
    }

    // Browser: the exact revision shows the stable diagnostic with no rows.
    const storageState = await context.storageState();
    const freshContext = await browser.newContext({ storageState });
    try {
      const page = await freshContext.newPage();
      await page.goto(
        getFrontendUrl(
          `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/${seed.revisionId}`,
        ),
        { waitUntil: "domcontentloaded" },
      );
      await expect(
        page.getByRole("heading", { level: 1, name: seed.compositionName }),
      ).toBeVisible();
      await expect(page.getByRole("alert")).toContainText(
        SOURCE_UNAVAILABLE_DIAGNOSTIC,
      );
      await expect(page.getByRole("table")).toHaveCount(0);
    } finally {
      await freshContext.close();
    }
  });
});
