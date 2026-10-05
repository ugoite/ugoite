import {
  type APIRequestContext,
  expect,
  test,
} from "@playwright/test";
import { getBackendUrl, getFrontendUrl, waitForServers } from "./lib/client.ts";

const SOURCE_UNAVAILABLE_DIAGNOSTIC = "A data source is unavailable.";

type SeedState = {
  spaceId: string;
  compositionId: string;
  revisionId: string;
  compositionName: string;
  canonicalYaml: string;
  missingFormId: string;
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

async function seedBrokenComposition(
  request: APIRequestContext,
): Promise<SeedState> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const compositionName = `Broken recovery ${suffix}`;
  // A never-created Form ID. The reference is shape-valid so regular save
  // accepts it; resolve must then report source_unavailable.
  const missingFormId = crypto.randomUUID();
  const yaml = [
    "format: ugoite.composition",
    "format_version: 1",
    `name: ${compositionName}`,
    "kind: dashboard",
    "tags: []",
    "spec:",
    "  sources:",
    "    - id: removed_form_rows",
    "      kind: entry_query",
    `      form_id: "${missingFormId}"`,
    "      query: {}",
    "  components:",
    "    - id: removed_form_table",
    "      kind: table",
    "      source: removed_form_rows",
    "  sections:",
    "    - id: main",
    "      components: [removed_form_table]",
    "",
  ].join("\n");
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
    missingFormId,
  };
}

test.describe("Composition Recovery Authorization", () => {
  let seed: SeedState;

  test.beforeAll(async ({ request }) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    seed = await seedBrokenComposition(request);
  });

  test("Composition raw recovery and ACL denial preserve caller-visible contracts", async ({ browser, context, playwright, request }) => {
    test.setTimeout(120_000);

    // Typed inspect (latest), the exact raw revision, and history preserve
    // the stored spec byte-identically.
    const latestResponse = await request.get(
      getBackendUrl(`/spaces/${seed.spaceId}/compositions/${seed.compositionId}`),
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

    // Resolve reports source_unavailable with no source metadata.
    const resolveResponse = await request.post(
      getBackendUrl(
        `/spaces/${seed.spaceId}/compositions/${seed.compositionId}/resolve`,
      ),
      { data: { revision_id: seed.revisionId, parameters: {} } },
    );
    expect(resolveResponse.ok()).toBe(true);
    const resolved = await resolveResponse.json();
    expect(resolved).toEqual({
      ok: false,
      parameter_definitions: [],
      diagnostics: [{ code: "source_unavailable" }],
    });
    expect(JSON.stringify(resolved)).not.toContain("removed_form_rows");
    expect(JSON.stringify(resolved)).not.toContain(seed.missingFormId);

    // A missing composition read returns the generic error shape.
    // Authenticated-denied reads share this shape by construction; that
    // boundary is covered by Rust storage/ACL tests, not by a second
    // principal here: generic_entry_mutations_cannot_write_composition_registry
    // plus the concealment unit tests in
    // crates/ugoite-core/src/composition.rs
    // (denied_metric_source_is_concealed_as_source_unavailable and
    // field_level_diagnostics_require_an_authorized_source_form_read).
    const missingResponse = await request.get(
      getBackendUrl(`/spaces/${seed.spaceId}/compositions/${crypto.randomUUID()}`),
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
