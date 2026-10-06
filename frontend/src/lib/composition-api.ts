import type { EntryPage, EntryPageRequest } from "./entry-query";
import type { CompositionStudioDocument } from "./composition-draft";
import type {
  EntryQueryCompositionDocument,
  EntryQueryCompositionFilter,
  EntryQueryCompositionProjection,
  EntryQueryCompositionSort,
} from "./entry-query-composition";
import type { SqlQueryPage, SqlQueryRequest, SqlResultColumn } from "./types";
import {
  canonicalizeCompositionDocument,
  type CompositionDocumentCanonicalization,
  type CompositionMetricPageEvaluation,
  type CompositionMetricPageRequest,
  entryApi,
  evaluateCompositionMetricPage,
  protocolFetch,
  sqlApi,
} from "./ugoite-client";
import type { SqlEntry } from "./types";

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
    /** `upsert`, `delete`, or `restore`. Absent on older cached shapes. */
    operation?: string;
  };
  fields: Record<string, unknown>;
  unmapped_field_values: Record<string, unknown>;
}

export interface CompositionHistoryPage {
  entry_id: string;
  revisions: CompositionRawRevision[];
  total: number;
  offset: number;
  limit: number;
  has_more: boolean;
}

export interface CompositionPublicationReceipt {
  command_id: string;
  catalog_generation: number;
  snapshot_id: number;
  committed_revision_ids: string[];
  committed_at_micros: number;
  data_file_count: number;
}

export interface CompositionLintDocument {
  format: "ugoite.composition";
  format_version: number;
  kind: string;
  name: string;
  tags: string[];
  spec: {
    parameters: Array<{
      id: string;
      label?: string;
      type: CompositionParameterType;
      required: boolean;
      default?: unknown;
      format?: "year-month";
    }>;
    sources: Array<{
      kind: string;
      id: string;
      entry_id?: string;
      revision_id?: string;
      form_id?: string;
      field_schema?: Array<{
        field_id: number;
        field_type: string;
        reference_form?: string;
        items?: { type: string; target_form?: string };
      }>;
      expected_result?: Array<{ name: string; type: CompositionResultType }>;
      variables?: Record<string, { parameter: string }>;
      query?: {
        text?: unknown;
        filters: EntryQueryCompositionFilter[];
        sort: EntryQueryCompositionSort[];
        page_limit?: number;
        projection: EntryQueryCompositionProjection;
      };
    }>;
    components: Array<{
      kind: string;
      id: string;
      label?: string;
      source: string;
      value_field?: { kind: string; field_id?: number; name?: string };
    }>;
    sections: Array<{ id: string; components: string[] }>;
  };
}

export interface CompositionLintResponse {
  ok: boolean;
  value?: {
    document: CompositionLintDocument;
    canonical_yaml: string;
    fingerprint: string;
  };
  error?: { kind: string; code: string };
}

export interface CompositionRestoreResponse {
  composition_id: string;
  revision_id: string;
  restored_from_revision_id: string;
  canonical_yaml: string;
  receipt: CompositionPublicationReceipt;
}

export interface CompositionSaveOptions {
  compositionId?: string;
  baseRevisionId?: string;
}

export interface CompositionSaveResponse {
  composition_id: string;
  revision_id: string;
  canonical_yaml: string;
  receipt: CompositionPublicationReceipt;
}

export interface SavedSqlCompositionDocument {
  format: "ugoite.composition";
  format_version: 1;
  kind: "dashboard";
  name: string;
  tags: string[];
  spec: {
    parameters: Array<{
      id: string;
      type: CompositionParameterType;
      required: true;
      default?: unknown;
    }>;
    sources: Array<{
      id: string;
      kind: "saved_sql";
      entry_id: string;
      revision_id: string;
      expected_result: Array<{ name: string; type: CompositionResultType }>;
      variables: Record<string, { parameter: string }>;
    }>;
    components: Array<{
      id: string;
      kind: "table";
      source: string;
    }>;
    sections: Array<{ id: string; components: string[] }>;
  };
}

export type CompositionSaveDocument =
  | SavedSqlCompositionDocument
  | EntryQueryCompositionDocument;

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

export interface CompositionPreviewPlan {
  draft_fingerprint: string;
  sources: CompositionResolvedSource[];
  component_bindings: CompositionResolvedComponentBinding[];
}

export interface CompositionPreviewResponse {
  ok: boolean;
  draft_fingerprint?: string;
  parameter_definitions?: CompositionParameterDefinition[];
  plan?: CompositionPreviewPlan;
  diagnostics?: CompositionResolveDiagnostic[];
}

export type CompositionSourcePage =
  | { kind: "entry_query"; page: EntryPage }
  | { kind: "saved_sql"; page: SqlQueryPage };

const compositionParameterTypes = new Set<CompositionParameterType>([
  "string",
  "boolean",
  "integer",
  "float",
  "date",
  "timestamp",
]);

export const canCreateSavedSqlComposition = (entry: SqlEntry): boolean =>
  entry.kind === "user-query" &&
  entry.variables.every((variable) =>
    compositionParameterTypes.has(variable.type as CompositionParameterType)
  );

/** Build the smallest typed Composition that retains one exact Saved SQL revision. */
export const buildSavedSqlCompositionDocument = (
  entry: SqlEntry,
  name: string,
  columns: readonly string[],
  parameterValues: Record<string, unknown>,
  resultSchema?: readonly SqlResultColumn[],
): SavedSqlCompositionDocument => {
  const parameters = entry.variables.map((variable) => {
    if (
      !compositionParameterTypes.has(variable.type as CompositionParameterType)
    ) {
      throw new Error(`Unsupported Saved SQL parameter type: ${variable.type}`);
    }
    const value = parameterValues[variable.name];
    return {
      id: variable.name,
      type: variable.type as CompositionParameterType,
      required: true as const,
      ...(value === null || value === undefined ? {} : { default: value }),
    };
  });
  const sourceId = "sql_results";
  const componentId = "results_table";
  // The descriptor stores the server-owned column types unchanged; columns
  // absent from the schema keep the previous json fallback. The Browser
  // never infers types from row values.
  const schemaByName = new Map(
    (resultSchema ?? []).map((column) => [column.name, column.type]),
  );
  return {
    format: "ugoite.composition",
    format_version: 1,
    kind: "dashboard",
    name: name.trim(),
    tags: [],
    spec: {
      parameters,
      sources: [{
        id: sourceId,
        kind: "saved_sql",
        entry_id: entry.id,
        revision_id: entry.revision_id,
        expected_result: columns.map((column) => ({
          name: column,
          type: schemaByName.get(column) ?? "json",
        })),
        variables: Object.fromEntries(
          entry.variables.map((variable) => [
            variable.name,
            { parameter: variable.name },
          ]),
        ),
      }],
      components: [{ id: componentId, kind: "table", source: sourceId }],
      sections: [{ id: "main", components: [componentId] }],
    },
  };
};

/** Thin browser adapter over the portable protocol and existing query paths. */
export const compositionApi = {
  async canonicalizeDocument(
    document: CompositionSaveDocument | CompositionStudioDocument,
  ): Promise<CompositionDocumentCanonicalization> {
    return await canonicalizeCompositionDocument(document);
  },

  async save(
    spaceId: string,
    yaml: string,
    idempotencyKey: string,
    opts?: CompositionSaveOptions,
    signal?: AbortSignal,
  ): Promise<CompositionSaveResponse> {
    return await protocolFetch<CompositionSaveResponse>(
      "composition.save",
      { space_id: spaceId, idempotency_key: idempotencyKey },
      {
        yaml,
        ...(opts?.compositionId === undefined
          ? {}
          : { composition_id: opts.compositionId }),
        ...(opts?.baseRevisionId === undefined
          ? {}
          : { base_revision_id: opts.baseRevisionId }),
      },
      { signal },
    );
  },

  /** Side-effect-free normalization of one stored spec through the server. */
  async lint(
    yaml: string,
    signal?: AbortSignal,
  ): Promise<CompositionLintResponse> {
    return await protocolFetch<CompositionLintResponse>(
      "composition.lint",
      {},
      { yaml },
      { signal },
    );
  },

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

  /** Bounded revision history for one Composition, oldest first. */
  async history(
    spaceId: string,
    compositionId: string,
    limit?: number,
    offset?: number,
    signal?: AbortSignal,
  ): Promise<CompositionHistoryPage> {
    return await protocolFetch<CompositionHistoryPage>(
      "composition.history",
      {
        space_id: spaceId,
        composition_id: compositionId,
        ...(limit === undefined ? {} : { limit }),
        ...(offset === undefined ? {} : { offset }),
      },
      undefined,
      { signal },
    );
  },

  /**
   * Restore one exact historical revision as a new append-only revision.
   * `baseRevisionId` must still be the current revision when the server
   * publishes; the idempotency key stays stable across uncertain retries
   * of the same source and base.
   */
  async restore(
    spaceId: string,
    compositionId: string,
    sourceRevisionId: string,
    baseRevisionId: string,
    idempotencyKey: string,
    signal?: AbortSignal,
  ): Promise<CompositionRestoreResponse> {
    return await protocolFetch<CompositionRestoreResponse>(
      "composition.restore",
      {
        space_id: spaceId,
        composition_id: compositionId,
        idempotency_key: idempotencyKey,
      },
      {
        source_revision_id: sourceRevisionId,
        base_revision_id: baseRevisionId,
      },
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

  /** Side-effect-free draft preview through the portable operation. */
  async preview(
    spaceId: string,
    yaml: string,
    parameters: Record<string, unknown>,
    signal?: AbortSignal,
  ): Promise<CompositionPreviewResponse> {
    return await protocolFetch<CompositionPreviewResponse>(
      "composition.preview",
      { space_id: spaceId },
      { yaml, parameters },
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
