import type { EntryPage, EntryPageRequest } from "./entry-query";
import type { SqlQueryPage, SqlQueryRequest } from "./types";
import {
  entryApi,
  evaluateCompositionMetricPage,
  protocolFetch,
  sqlApi,
  type CompositionMetricPageEvaluation,
  type CompositionMetricPageRequest,
} from "./ugoite-client";

export interface CompositionListItem {
  composition_id: string;
  revision_id: string;
  updated_at: number;
  name?: unknown;
  kind?: unknown;
  format_version?: unknown;
  tags: string[];
}

export interface CompositionListPage {
  items: CompositionListItem[];
  offset: number;
  limit: number;
  has_more: boolean;
}

export interface CompositionRawRevision {
  revision: {
    entry_id: string;
    revision_id: string;
    committed_at_micros: number;
  };
  fields: Record<string, unknown>;
  unmapped_field_values: Record<string, unknown>;
}

export type CompositionParameterType =
  | "string"
  | "boolean"
  | "integer"
  | "float"
  | "date"
  | "timestamp";

export interface CompositionParameterDefinition {
  id: string;
  label?: string;
  type: CompositionParameterType;
  required: boolean;
  default?: unknown;
  format?: "year-month";
}

export type CompositionResultType =
  | "string"
  | "boolean"
  | "integer"
  | "float"
  | "date"
  | "timestamp"
  | "json";

export interface CompositionResolveDiagnostic {
  code: string;
  parameter_id?: string;
}

export interface CompositionResolvePlan {
  composition_revision: { entry_id: string; revision_id: string };
  sources: CompositionResolvedSource[];
  component_bindings: CompositionResolvedComponentBinding[];
}

export type CompositionResolvedSource =
  | {
    kind: "entry_query";
    source_id: string;
    request: EntryPageRequest;
    source_schema_fingerprint: string;
  }
  | {
    kind: "saved_sql";
    source_id: string;
    request: SqlQueryRequest;
    source_schema_fingerprint: string;
  };

interface CompositionResolvedComponentBase {
  component_id: string;
  label?: string;
  source_id: string;
}

export type CompositionResolvedComponentBinding =
  | (CompositionResolvedComponentBase & {
    kind: "metric";
    metric_field_id?: number;
    result_property_key?: string;
    expected_result_type: CompositionResultType;
  })
  | (CompositionResolvedComponentBase & { kind: "table" });

export interface CompositionResolveResponse {
  ok: boolean;
  parameter_definitions?: CompositionParameterDefinition[];
  plan?: CompositionResolvePlan;
  diagnostics?: CompositionResolveDiagnostic[];
}

export type CompositionSourcePage =
  | { kind: "entry_query"; page: EntryPage }
  | { kind: "saved_sql"; page: SqlQueryPage };

/** Thin browser adapter over the portable protocol and existing query paths. */
export const compositionApi = {
  async list(
    spaceId: string,
    limit = 100,
    offset = 0,
    signal?: AbortSignal,
  ): Promise<CompositionListPage> {
    return await protocolFetch<CompositionListPage>(
      "composition.list",
      { space_id: spaceId, limit, offset },
      undefined,
      { signal },
    );
  },

  async get(
    spaceId: string,
    compositionId: string,
    revisionId: string,
    signal?: AbortSignal,
  ): Promise<CompositionRawRevision> {
    return await protocolFetch<CompositionRawRevision>(
      "composition.get",
      {
        space_id: spaceId,
        composition_id: compositionId,
        revision_id: revisionId,
      },
      undefined,
      { signal },
    );
  },

  async resolve(
    spaceId: string,
    compositionId: string,
    revisionId: string,
    parameters: Record<string, unknown>,
    signal?: AbortSignal,
  ): Promise<CompositionResolveResponse> {
    return await protocolFetch<CompositionResolveResponse>(
      "composition.resolve",
      { space_id: spaceId, composition_id: compositionId },
      { revision_id: revisionId, parameters },
      { signal },
    );
  },

  async evaluateMetricPage(
    request: CompositionMetricPageRequest,
  ): Promise<CompositionMetricPageEvaluation> {
    return await evaluateCompositionMetricPage(request);
  },

  async querySource(
    spaceId: string,
    source: CompositionResolvedSource,
    cursor: string | undefined,
    signal: AbortSignal,
  ): Promise<CompositionSourcePage> {
    if (source.kind === "entry_query") {
      const { after: _resolvedCursor, ...request } = source.request;
      const page = await entryApi.query(spaceId, {
        ...request,
        ...(cursor === undefined ? {} : { after: cursor }),
      }, signal);
      return { kind: "entry_query", page };
    }

    const { continuation: _resolvedCursor, ...request } = source.request;
    const page = await sqlApi.query(spaceId, {
      ...request,
      ...(cursor === undefined ? {} : { continuation: cursor }),
    }, signal);
    return { kind: "saved_sql", page };
  },
};

export const compositionDisplayName = (value: unknown): string =>
  typeof value === "string" && value.trim() ? value.trim() : "Composition";

export type { CompositionMetricPageEvaluation, CompositionMetricPageRequest };
