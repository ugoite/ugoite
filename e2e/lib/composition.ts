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
