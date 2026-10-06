import { type APIRequestContext, expect, test } from "@playwright/test";
import { getBackendUrl, waitForServers } from "./lib/client.ts";

type SeedState = {
  spaceId: string;
  yaml: string;
  renamedYaml: string;
};

type CompositionSaveResponse = {
  composition_id: string;
  revision_id: string;
  canonical_yaml: string;
  receipt: {
    command_id: string;
    catalog_generation: number;
    snapshot_id: number;
    committed_revision_ids: string[];
    committed_at_micros: number;
    data_file_count: number;
  };
};

type CompositionHistoryPage = {
  total?: number;
  revisions?: Array<{ revision: { revision_id: string } }>;
};

function sqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

function buildYaml(
  name: string,
  savedSqlId: string,
  savedSqlRevisionId: string,
): string {
  return [
    "format: ugoite.composition",
    "format_version: 1",
    `name: ${name}`,
    "kind: dashboard",
    "tags: []",
    "spec:",
    "  sources:",
    "    - id: receipt_rows",
    "      kind: saved_sql",
    `      entry_id: "${savedSqlId}"`,
    `      revision_id: "${savedSqlRevisionId}"`,
    "      expected_result:",
    "        - name: total",
    "          type: float",
    "        - name: merchant",
    "          type: string",
    "  components:",
    "    - id: total",
    "      kind: metric",
    "      source: receipt_rows",
    "      value_field:",
    "        kind: sql_column",
    "        name: total",
    "    - id: rows",
    "      kind: table",
    "      source: receipt_rows",
    "  layout:",
    "    kind: flow",
    "    rows:",
    "      - id: main",
    "        items:",
    "          - kind: component",
    "            component: total",
    "          - kind: component",
    "            component: rows",
    "",
  ].join("\n");
}

async function createSpace(request: APIRequestContext): Promise<string> {
  const suffix = `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
  const response = await request.post(getBackendUrl("/spaces"), {
    data: {
      slug: `composition-receipt-${suffix}`,
      name: `Composition Receipt ${suffix}`,
    },
  });
  expect([200, 201]).toContain(response.status());
  const result = await response.json() as { space_uid?: string };
  expect(result.space_uid).toBeTruthy();
  return result.space_uid!;
}

async function seedReceiptComposition(
  request: APIRequestContext,
): Promise<SeedState> {
  const spaceId = await createSpace(request);
  const suffix = crypto.randomUUID().slice(0, 8);
  const formName = `ReceiptScalar${suffix}`;
  const compositionName = `Receipt scalar ${suffix}`;

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
          Date: "2026-05-10",
          Merchant: "Receipt-One",
          Amount: 9,
        },
      },
    },
  );
  expect(entryResponse.status()).toBe(201);

  const sqlResponse = await request.post(
    getBackendUrl(`/spaces/${spaceId}/sql`),
    {
      data: {
        name: `Receipt scalar query ${suffix}`,
        kind: "user-query",
        sql: `SELECT ${sqlIdentifier(amountColumn!)} AS total, ${
          sqlIdentifier(merchantColumn!)
        } AS merchant FROM ${sqlIdentifier(relation!)} ORDER BY ${
          sqlIdentifier(merchantColumn!)
        } ASC`,
        variables: [],
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

  return {
    spaceId,
    yaml: buildYaml(compositionName, savedSql.id!, savedSql.revision_id!),
    renamedYaml: buildYaml(
      `${compositionName} renamed`,
      savedSql.id!,
      savedSql.revision_id!,
    ),
  };
}

test.describe("Composition Save Receipt", () => {
  let seed: SeedState;

  test.beforeAll(async ({ request }) => {
    test.setTimeout(120_000);
    await waitForServers(request);
    seed = await seedReceiptComposition(request);
  });

  test("Composition save reconciles a lost response without duplicate publication", async ({ request }) => {
    test.setTimeout(120_000);
    const saveUrl = getBackendUrl(`/spaces/${seed.spaceId}/compositions`);
    const key = crypto.randomUUID();

    const firstResponse = await request.post(saveUrl, {
      headers: { "Idempotency-Key": key },
      data: { yaml: seed.yaml },
    });
    expect(firstResponse.status()).toBe(201);
    const created = await firstResponse.json() as CompositionSaveResponse;
    expect(created.composition_id).toBeTruthy();
    expect(created.revision_id).toBeTruthy();
    expect(created.receipt.committed_revision_ids).toContain(
      created.revision_id,
    );

    // The client loses the first response and retries with the same key and
    // the identical yaml: reconciliation returns the original receipt with no
    // duplicate publication.
    const replayResponse = await request.post(saveUrl, {
      headers: { "Idempotency-Key": key },
      data: { yaml: seed.yaml },
    });
    expect(replayResponse.status()).toBe(firstResponse.status());
    const reconciled = await replayResponse.json() as CompositionSaveResponse;
    expect(reconciled).toEqual(created);

    // The same key with different content is an idempotency conflict.
    const conflictResponse = await request.post(saveUrl, {
      headers: { "Idempotency-Key": key },
      data: { yaml: seed.renamedYaml },
    });
    expect(conflictResponse.status()).toBe(409);
    const conflict = await conflictResponse.json() as { code?: unknown };
    expect(conflict.code).toBe("IDEMPOTENCY_CONFLICT");

    // Exactly one publication exists.
    const historyResponse = await request.get(
      getBackendUrl(
        `/spaces/${seed.spaceId}/compositions/${created.composition_id}/history?limit=100&offset=0`,
      ),
    );
    expect(historyResponse.ok()).toBe(true);
    const history = await historyResponse.json() as CompositionHistoryPage;
    expect(history.total).toBe(1);
    expect(history.revisions).toHaveLength(1);
    expect(history.revisions![0]!.revision.revision_id).toBe(
      created.revision_id,
    );
  });
});
