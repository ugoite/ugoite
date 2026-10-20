import { type APIRequestContext, expect } from "@playwright/test";
import { getBackendUrl } from "./client.ts";

type GraphVariable = Record<string, unknown> & {
  id: string;
  kind: "constant" | "input" | "query";
  query?: Record<string, unknown>;
};

export type CompositionGraphSeed = {
  name: string;
  variables: GraphVariable[];
  components: Array<Record<string, unknown>>;
  rows: Array<{
    id: string;
    items: Array<{ kind: "component"; component: string }>;
  }>;
};

type PreviewQuery = {
  variable_id: string;
  returns: unknown;
  form_bindings: Array<{ relation: string; form_id: string }>;
  schema_fingerprint: string;
};

type CompositionPreview = {
  ok: boolean;
  plan?: { queries: PreviewQuery[] };
  diagnostics?: Array<{ code?: string; variable_id?: string }>;
};

type CompositionSaveResponse = {
  composition_id: string;
  revision_id: string;
};

/** Encode the strict graph DTO as YAML with JSON flow collections. */
export function compositionGraphYaml(seed: CompositionGraphSeed): string {
  return [
    "format: ugoite.composition",
    "format_version: 1",
    "kind: dashboard",
    `name: ${JSON.stringify(seed.name)}`,
    "spec:",
    `  variables: ${JSON.stringify(seed.variables)}`,
    `  components: ${JSON.stringify(seed.components)}`,
    `  layout: ${JSON.stringify({ kind: "flow", rows: seed.rows })}`,
    "",
  ].join("\n");
}

/** Ask Core to derive every query return, Form binding, and schema fingerprint. */
export async function completeCompositionGraphYaml(
  request: APIRequestContext,
  spaceId: string,
  seed: CompositionGraphSeed,
): Promise<string> {
  const draft = compositionGraphYaml(seed);
  const response = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions/preview`),
    { data: { yaml: draft } },
  );
  const body = await response.text();
  expect(response.status(), body).toBe(200);
  const preview = JSON.parse(body) as CompositionPreview;
  expect(preview.ok, JSON.stringify(preview.diagnostics)).toBe(true);
  const queries = new Map(
    (preview.plan?.queries ?? []).map((query) => [query.variable_id, query]),
  );
  const variables = seed.variables.map((variable) => {
    if (variable.kind !== "query") return variable;
    const planned = queries.get(variable.id);
    expect(planned, `Missing preview for query variable ${variable.id}`)
      .toBeTruthy();
    expect(planned!.schema_fingerprint).toMatch(/^[0-9a-f]{64}$/);
    return {
      ...variable,
      returns: planned!.returns,
      query: {
        ...variable.query,
        form_bindings: planned!.form_bindings,
        schema_fingerprint: planned!.schema_fingerprint,
      },
    };
  });
  expect(queries.size).toBe(
    seed.variables.filter((item) => item.kind === "query").length,
  );
  return compositionGraphYaml({ ...seed, variables });
}

export async function saveCompositionGraph(
  request: APIRequestContext,
  spaceId: string,
  yaml: string,
): Promise<CompositionSaveResponse> {
  const response = await request.post(
    getBackendUrl(`/spaces/${spaceId}/compositions`),
    {
      headers: { "Idempotency-Key": crypto.randomUUID() },
      data: { yaml },
    },
  );
  const body = await response.text();
  expect([200, 201], body).toContain(response.status());
  const saved = JSON.parse(body) as CompositionSaveResponse;
  expect(saved.composition_id).toBeTruthy();
  expect(saved.revision_id).toBeTruthy();
  return saved;
}
