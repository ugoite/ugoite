import { type APIRequestContext, expect } from "@playwright/test";
import { getBackendUrl } from "./client.ts";

type CompositionResultColumn = {
  name: string;
  type: string;
};

type CompositionPreviewResponse = {
  ok: boolean;
  plan?: {
    sources: Array<{
      source_id: string;
      source_schema_fingerprint: string;
    }>;
  };
};

type SavedSqlVariable = {
  type: "string" | "boolean" | "integer" | "float" | "date" | "timestamp";
  name: string;
};

type SavedSqlResponse = {
  id?: string;
  revision_id?: string;
  sql?: string;
  variables?: SavedSqlVariable[];
};

type SqlResultSchemaColumn = {
  name: string;
  type: CompositionResultColumn["type"];
};

type SqlQuerySchemaResponse = {
  result_schema?: SqlResultSchemaColumn[];
};

export type SavedSqlCompositionSourceRef = {
  id: string;
  savedSqlId: string;
  savedSqlRevisionId: string;
  /** Optional query values for SQL that requires a particular valid sample. */
  sampleParameters?: Record<string, string | number | boolean>;
};

function sampleParameterValue(
  type: SavedSqlVariable["type"],
): string | number | boolean {
  switch (type) {
    case "string":
      return "composition-schema-probe";
    case "boolean":
      return true;
    case "integer":
      return 1;
    case "float":
      return 1.25;
    case "date":
      return "2026-01-15";
    case "timestamp":
      return "2026-01-15T00:00:00Z";
  }
}

async function compositionSchemaFingerprint(
  columns: CompositionResultColumn[],
): Promise<string> {
  const canonical = new TextEncoder().encode(JSON.stringify(columns));
  const digest = await crypto.subtle.digest("SHA-256", canonical);
  return Array.from(
    new Uint8Array(digest),
    (byte) => byte.toString(16).padStart(2, "0"),
  ).join("");
}

/**
 * Resolve exact Saved SQL revisions through the query API and fill their
 * server-owned result schema before saving a SQL-first Composition.
 */
export async function completeSavedSqlCompositionYaml(
  request: APIRequestContext,
  spaceId: string,
  draftYaml: string,
  sources: SavedSqlCompositionSourceRef[],
): Promise<string> {
  const lines = draftYaml.split("\n");

  for (const source of sources) {
    const savedSqlResponse = await request.get(
      getBackendUrl(`/spaces/${spaceId}/sql/${source.savedSqlId}`),
    );
    const savedSqlBody = await savedSqlResponse.text();
    expect(savedSqlResponse.status(), savedSqlBody).toBe(200);
    const savedSql = JSON.parse(savedSqlBody) as SavedSqlResponse;
    expect(savedSql.id).toBe(source.savedSqlId);
    expect(savedSql.revision_id).toBe(source.savedSqlRevisionId);
    expect(savedSql.sql).toBeTruthy();

    const parameters = Object.fromEntries(
      (savedSql.variables ?? []).map((variable) => [
        variable.name,
        source.sampleParameters?.[variable.name] ??
          sampleParameterValue(variable.type),
      ]),
    );
    const parameterTypes = Object.fromEntries(
      (savedSql.variables ?? []).map((variable) => [
        variable.name,
        variable.type,
      ]),
    );
    const queryResponse = await request.post(
      getBackendUrl(`/spaces/${spaceId}/sql/query`),
      {
        data: {
          sql: savedSql.sql,
          saved_sql: {
            id: source.savedSqlId,
            revision_id: source.savedSqlRevisionId,
          },
          parameters,
          parameter_types: parameterTypes,
          limit: 1,
        },
      },
    );
    const queryBody = await queryResponse.text();
    expect(queryResponse.status(), queryBody).toBe(200);
    const query = JSON.parse(queryBody) as SqlQuerySchemaResponse;
    expect(query.result_schema?.length).toBeGreaterThan(0);
    const expectedResult = query.result_schema!.map(({ name, type }) => ({
      name,
      type,
    }));
    const fingerprint = await compositionSchemaFingerprint(expectedResult);

    const sourceStart = lines.findIndex((line) =>
      line === `    - id: ${source.id}`
    );
    expect(sourceStart).toBeGreaterThanOrEqual(0);
    const sourceEnd = lines.findIndex((line, index) =>
      index > sourceStart &&
      (line.startsWith("    - id: ") || line === "  components:")
    );
    const end = sourceEnd < 0 ? lines.length : sourceEnd;
    const sourceLines = lines.slice(sourceStart, end);
    const expectedResultIndex = sourceLines.findIndex((line) =>
      line === "      expected_result: []"
    );
    const fingerprintIndex = sourceLines.findIndex((line) =>
      line === "      schema_fingerprint: ''"
    );
    expect(expectedResultIndex).toBeGreaterThanOrEqual(0);
    expect(fingerprintIndex).toBeGreaterThanOrEqual(0);
    lines[sourceStart + expectedResultIndex] = `      expected_result: ${
      JSON.stringify(expectedResult)
    }`;
    lines[sourceStart + fingerprintIndex] = `      schema_fingerprint: ${
      JSON.stringify(fingerprint)
    }`;
  }

  return lines.join("\n");
}

/** Complete the draft-only SQL source tuple with the server's current schema. */
export async function completeInlineSqlCompositionYaml(
  request: APIRequestContext,
  spaceId: string,
  draftYaml: string,
  source: {
    id: string;
    relation: string;
    formId: string;
    expectedResult: CompositionResultColumn[];
  },
): Promise<string> {
  const response = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions/preview`),
    { data: { yaml: draftYaml } },
  );
  expect(response.status(), await response.text()).toBe(200);
  const preview = await response.json() as CompositionPreviewResponse;
  expect(preview.ok).toBe(true);
  const resolved = preview.plan?.sources.find((item) =>
    item.source_id === source.id
  );
  expect(resolved?.source_schema_fingerprint).toMatch(/^[0-9a-f]{64}$/);

  let yaml = draftYaml;
  const replaceLine = (line: string, replacement: string) => {
    expect(yaml).toContain(line);
    yaml = yaml.replace(line, replacement);
  };
  replaceLine(
    "      form_bindings: []",
    `      form_bindings: ${
      JSON.stringify([{
        relation: source.relation,
        form_id: source.formId,
      }])
    }`,
  );
  replaceLine(
    "      expected_result: []",
    `      expected_result: ${JSON.stringify(source.expectedResult)}`,
  );
  replaceLine(
    "      schema_fingerprint: ''",
    `      schema_fingerprint: ${
      JSON.stringify(
        resolved!.source_schema_fingerprint,
      )
    }`,
  );
  return yaml;
}
