import type { CompositionStudioDocument } from "./composition-draft";
import type {
  SqlQueryPage,
  SqlQueryRequest,
  SqlResultColumn,
  SqlResultColumnType,
} from "./types";
import {
  canonicalizeCompositionDocument,
  type CompositionDocumentCanonicalization,
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
  spec: {
    variables?: Array<{
      id: string;
      type: CompositionVariableType;
      required: boolean;
      default?: unknown;
      format?: "year-month";
    }>;
    sources: CompositionStudioDocument["spec"]["sources"];
    components: Array<
      | {
        kind: "input";
        id: string;
        variable: string;
      }
      | {
        kind: "metric";
        id: string;
        source: string;
        column: string;
        presentation?: CompositionMetricPresentation;
      }
      | {
        kind: "table";
        id: string;
        source: string;
      }
      | {
        kind: "text";
        id: string;
        content:
          | { kind: "literal"; text: string }
          | { kind: "variable"; variable: string };
        style: CompositionTextStyle;
      }
    >;
    layout: CompositionFlowLayout;
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

export type CompositionTextStyle = "title" | "heading" | "body" | "caption";

export interface CompositionFlowLayoutItem {
  kind: "component";
  component: string;
}

export interface CompositionFlowLayoutRow {
  id: string;
  items: CompositionFlowLayoutItem[];
}

export interface CompositionFlowLayout {
  kind: "flow";
  rows: CompositionFlowLayoutRow[];
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
  spec: {
    variables: Array<{
      id: string;
      type: CompositionVariableType;
      required: true;
      default?: unknown;
    }>;
    sources: Array<{
      id: string;
      kind: "saved_sql";
      entry_id: string;
      revision_id: string;
      expected_result: Array<{ name: string; type: CompositionResultType }>;
      variables: Record<string, { variable: string }>;
    }>;
    components: Array<
      | { id: string; kind: "input"; variable: string }
      | { id: string; kind: "table"; source: string }
    >;
    layout: {
      kind: "flow";
      rows: Array<{
        id: string;
        items: Array<{ kind: "component"; component: string }>;
      }>;
    };
  };
}

export type CompositionSaveDocument = CompositionStudioDocument;

export type CompositionVariableType =
  | "string"
  | "boolean"
  | "integer"
  | "float"
  | "date"
  | "timestamp";

export interface CompositionVariableDefinition {
  id: string;
  type: CompositionVariableType;
  required: boolean;
  default?: unknown;
  format?: "year-month";
}

export type CompositionResultType =
  | "string"
  | "boolean"
  | "integer"
  | "float"
  | "decimal"
  | "date"
  | "timestamp"
  | "json";

/**
 * Keep SQL's server-owned schema descriptor separate from the Composition
 * source grammar. A SQL result type becomes a Composition type only after the
 * Domain and portable protocol admit it; unknown types retain the JSON
 * fallback rather than producing a document the server cannot read.
 */
export function compositionResultTypeFromSql(
  resultType: SqlResultColumnType | undefined,
): CompositionResultType {
  switch (resultType) {
    case "string":
    case "boolean":
    case "integer":
    case "float":
    case "decimal":
    case "date":
    case "timestamp":
    case "json":
      return resultType;
    default:
      return "json";
  }
}

export type CompositionMetricPresentation = {
  kind: "number";
  unit?: string;
  fraction_digits?: number;
};

export interface CompositionResolveDiagnostic {
  code: string;
  variable_id?: string;
}

/** Authoritative dependency edges produced by the Core resolver. */
export interface CompositionDependencyIndex {
  variable_consumers?: Record<string, {
    input_component_ids?: string[];
    text_component_ids?: string[];
    source_ids?: string[];
  }>;
  source_consumers?: Record<string, {
    metric_component_ids?: string[];
    table_component_ids?: string[];
  }>;
}

export interface CompositionResolvePlan {
  composition_revision: { entry_id: string; revision_id: string };
  sources: CompositionResolvedSource[];
  component_bindings: CompositionResolvedComponentBinding[];
  dependencies?: CompositionDependencyIndex;
}

export type CompositionResolvedSource =
  | {
    kind: "inline_sql";
    source_id: string;
    request: SqlQueryRequest;
    source_schema_fingerprint: string;
    form_bindings: Array<{ relation: string; form_id: string }>;
    expected_result: Array<{ name: string; type: CompositionResultType }>;
  }
  | {
    kind: "saved_sql";
    source_id: string;
    request: SqlQueryRequest;
    source_schema_fingerprint: string;
    expected_result: Array<{ name: string; type: CompositionResultType }>;
  };

interface CompositionResolvedComponentBase {
  component_id: string;
  source_id?: string;
  variable_id?: string;
}

export type CompositionResolvedComponentBinding =
  | (CompositionResolvedComponentBase & {
    kind: "input";
    variable_id: string;
    source_id?: undefined;
  })
  | (CompositionResolvedComponentBase & {
    kind: "metric";
    /** Metric and table bindings always carry their source request identity. */
    source_id: string;
    result_property_key?: string;
    expected_result_type: CompositionResultType;
    presentation?: CompositionMetricPresentation;
  })
  | (CompositionResolvedComponentBase & {
    kind: "table";
    /** Metric and table bindings always carry their source request identity. */
    source_id: string;
  })
  | (CompositionResolvedComponentBase & {
    kind: "text";
    /**
     * Text bindings carry no source binding and trigger no source request.
     * The renderer joins them with their component declaration by
     * `component_id` for content and style.
     */
    source_id?: undefined;
  });

export interface CompositionResolveResponse {
  ok: boolean;
  variable_definitions?: CompositionVariableDefinition[];
  plan?: CompositionResolvePlan;
  diagnostics?: CompositionResolveDiagnostic[];
}

export interface CompositionPreviewPlan {
  draft_fingerprint: string;
  sources: CompositionResolvedSource[];
  component_bindings: CompositionResolvedComponentBinding[];
  dependencies?: CompositionDependencyIndex;
}

export interface CompositionPreviewResponse {
  ok: boolean;
  draft_fingerprint?: string;
  variable_definitions?: CompositionVariableDefinition[];
  plan?: CompositionPreviewPlan;
  diagnostics?: CompositionResolveDiagnostic[];
}

export interface CompositionRevisionBase {
  entryId: string;
  revisionId: string;
}

export type CompositionMetricQueryTarget =
  | {
    kind: "saved_revision";
    composition_id: string;
    revision_id: string;
  }
  | { kind: "draft"; yaml: string };

export interface CompositionMetricQueryRequest {
  target: CompositionMetricQueryTarget;
  base_entry_id?: string;
  base_revision_id?: string;
  component_id: string;
  variables: Record<string, unknown>;
  unset_variables?: string[];
}

export interface CompositionMetricQueryValue {
  component_id: string;
  result_type: CompositionResultType | "decimal";
  value: unknown;
}

export interface CompositionMetricQueryDiagnostic {
  code: string;
  component_id?: string;
  variable_id?: string;
}

export interface CompositionMetricQueryResponse {
  ok: boolean;
  draft_fingerprint?: string;
  result?: CompositionMetricQueryValue;
  diagnostics?: CompositionMetricQueryDiagnostic[];
}

const splitVariableSnapshot = (
  snapshot: Record<string, unknown>,
): { variables: Record<string, unknown>; unset_variables: string[] } => {
  const variables: Record<string, unknown> = {};
  const unset_variables: string[] = [];
  for (const [id, value] of Object.entries(snapshot)) {
    if (value === null) unset_variables.push(id);
    else variables[id] = value;
  }
  return { variables, unset_variables };
};

export type CompositionSourcePage =
  | { kind: "sql"; page: SqlQueryPage };

const compositionVariableTypes = new Set<CompositionVariableType>([
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
    compositionVariableTypes.has(variable.type as CompositionVariableType)
  );

/** Build a Table-only Composition for an exact Saved SQL revision. */
export const buildSavedSqlCompositionDocument = (
  entry: SqlEntry,
  name: string,
  columns: readonly string[],
  resultSchema?: readonly SqlResultColumn[],
): SavedSqlCompositionDocument => {
  for (const variable of entry.variables) {
    if (
      !compositionVariableTypes.has(variable.type as CompositionVariableType)
    ) {
      throw new Error(`Unsupported Saved SQL variable type: ${variable.type}`);
    }
  }
  const sourceId = "sql_results";
  const componentId = "results_table";
  // Keep the server descriptor authoritative and map only result types that
  // Composition represents explicitly. Unknown types use the JSON fallback.
  const schemaByName = new Map(
    (resultSchema ?? []).map((column) => [column.name, column.type]),
  );
  return {
    format: "ugoite.composition",
    format_version: 1,
    kind: "dashboard",
    name: name.trim(),
    spec: {
      variables: [],
      sources: [{
        id: sourceId,
        kind: "saved_sql",
        entry_id: entry.id,
        revision_id: entry.revision_id,
        expected_result: columns.map((column) => ({
          name: column,
          type: compositionResultTypeFromSql(schemaByName.get(column)),
        })),
        variables: {},
      }],
      components: [{ id: componentId, kind: "table", source: sourceId }],
      layout: {
        kind: "flow",
        rows: [{
          id: "main",
          items: [{ kind: "component", component: componentId }],
        }],
      },
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
    variables: Record<string, unknown>,
    signal?: AbortSignal,
  ): Promise<CompositionResolveResponse> {
    const snapshot = splitVariableSnapshot(variables);
    return await protocolFetch<CompositionResolveResponse>(
      "composition.resolve",
      { space_id: spaceId, composition_id: compositionId },
      {
        revision_id: revisionId,
        variables: snapshot.variables,
        ...(snapshot.unset_variables.length > 0
          ? { unset_variables: snapshot.unset_variables }
          : {}),
      },
      { signal },
    );
  },

  /** Side-effect-free draft preview through the portable operation. */
  async preview(
    spaceId: string,
    yaml: string,
    variables: Record<string, unknown>,
    signal?: AbortSignal,
    base?: CompositionRevisionBase,
  ): Promise<CompositionPreviewResponse> {
    const snapshot = splitVariableSnapshot(variables);
    return await protocolFetch<CompositionPreviewResponse>(
      "composition.preview",
      { space_id: spaceId },
      {
        yaml,
        ...(base
          ? {
            base_entry_id: base.entryId,
            base_revision_id: base.revisionId,
          }
          : {}),
        variables: snapshot.variables,
        ...(snapshot.unset_variables.length > 0
          ? { unset_variables: snapshot.unset_variables }
          : {}),
      },
      { signal },
    );
  },

  /** Execute one metric through the bounded, current-authorized operation. */
  async queryMetric(
    spaceId: string,
    request: CompositionMetricQueryRequest,
    signal?: AbortSignal,
  ): Promise<CompositionMetricQueryResponse> {
    const { variables, unset_variables: explicitlyUnset = [], ...requestBody } =
      request;
    const snapshot = splitVariableSnapshot(variables);
    const unset_variables = [
      ...new Set([...explicitlyUnset, ...snapshot.unset_variables]),
    ];
    return await protocolFetch<CompositionMetricQueryResponse>(
      "composition.metric.query",
      { space_id: spaceId },
      {
        ...requestBody,
        variables: snapshot.variables,
        ...(unset_variables.length > 0 ? { unset_variables } : {}),
      },
      { signal },
    );
  },

  async querySource(
    spaceId: string,
    source: CompositionResolvedSource,
    cursor: string | undefined,
    signal: AbortSignal,
  ): Promise<CompositionSourcePage> {
    const { continuation: _resolvedCursor, ...request } = source.request;
    const page = await sqlApi.query(spaceId, {
      ...request,
      include_exact_integer_values: true,
      ...(cursor === undefined ? {} : { continuation: cursor }),
    }, signal);
    return { kind: "sql", page };
  },

  async countSource(
    spaceId: string,
    source: CompositionResolvedSource,
    signal?: AbortSignal,
  ): Promise<number> {
    const { continuation: _continuation, ...request } = source.request;
    return await sqlApi.count(spaceId, {
      sql: request.sql,
      parameters: request.parameters,
      parameter_types: request.parameter_types,
      ...(request.saved_sql ? { saved_sql: request.saved_sql } : {}),
    }, signal);
  },
};

export const compositionDisplayName = (value: unknown): string =>
  typeof value === "string" && value.trim() ? value.trim() : "Composition";
