import type {
  CompositionGraphDocumentDraft,
  CompositionLiteral,
  CompositionQueryReturn,
  CompositionScalarDescriptor,
} from "./composition-variable-binding";
import {
  canonicalizeCompositionDocument,
  type CompositionDocumentCanonicalization,
  protocolFetch,
} from "./ugoite-client";
import type { SqlEntry, SqlQueryRequest } from "./types";

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

export type CompositionLintDocument = CompositionGraphDocumentDraft;

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
  variables?: Record<string, unknown>;
  unset_variables?: string[];
}

/** Authorized graph query metadata returned by the existing preview operation. */
export interface CompositionGraphPreviewQuery {
  variable_id: string;
  request?: SqlQueryRequest;
  /** Source of the authorized exact Saved SQL revision, never the latest. */
  sql_text?: string;
  /** Rebound by Rust to immutable Form relation IDs, without granting authority. */
  portable_sql?: string;
  deferred_arguments: Array<{
    argument_name: string;
    variable_id: string;
    value_type: CompositionScalarDescriptor;
  }>;
  returns: CompositionQueryReturn;
  result_columns: Array<{
    name: string;
    value_type: CompositionScalarDescriptor;
  }>;
  form_bindings: Array<{ relation: string; form_id: string }>;
  schema_fingerprint: string;
  query_semantic_fingerprint: string;
  scalar_cardinality?: "exactly_one_row";
}

export interface CompositionGraphPreviewResponse {
  ok: boolean;
  draft_fingerprint?: string;
  variable_definitions?: Array<{
    id: string;
    value_type: CompositionScalarDescriptor;
    required: boolean;
    default?: unknown;
  }>;
  plan?: {
    queries: CompositionGraphPreviewQuery[];
    dependencies?: CompositionDependencyIndex;
  };
  diagnostics?: Array<{ code?: string; message?: string }>;
}

export type CompositionEvaluationTarget =
  | {
    kind: "saved_revision";
    composition_id: string;
    revision_id: string;
  }
  | { kind: "draft"; yaml: string };

export interface CompositionEvaluationRequest {
  evaluation_id: string;
  target: CompositionEvaluationTarget;
  base_entry_id?: string;
  base_revision_id?: string;
  variables: Record<string, unknown>;
  unset_variables?: string[];
  page?: { variable_id: string; cursor: string };
  /** Query graph roots to recompute; Core expands and validates dependencies. */
  requested_queries?: string[];
}

export type CompositionEvaluatedResult =
  | { kind: "scalar"; value: CompositionLiteral }
  | {
    kind: "table";
    columns: Array<{
      name: string;
      value_type: CompositionScalarDescriptor;
    }>;
    rows: CompositionLiteral[][];
  };

export interface CompositionEvaluationResponse {
  evaluation_id: string;
  ok: boolean;
  snapshot_fingerprint?: string;
  publication_fingerprint?: string;
  authorization_fingerprint?: string;
  computed_query_ids: string[];
  composition_revision?: { entry_id: string; revision_id: string };
  draft_fingerprint?: string;
  variable_values: Record<string, CompositionLiteral>;
  queries: CompositionGraphPreviewQuery[];
  component_bindings: Array<{
    component_id: string;
    kind: string;
    variable_id?: string;
    expected_result_type?: CompositionScalarDescriptor["scalar_type"];
    presentation?: Record<string, unknown>;
  }>;
  dependencies: CompositionDependencyIndex;
  query_results: Record<string, {
    value: CompositionEvaluatedResult;
    has_more: boolean;
    next?: string;
  }>;
  diagnostics?: Array<
    { code?: string; message?: string; variable_id?: string }
  >;
  query_diagnostics?: Array<{
    code: string;
    variable_id: string;
    request_diagnostic?: { code: string; argument_name?: string };
    result_diagnostic?: { code: string; result_diagnostic?: { code: string } };
  }>;
}

/** Core-owned graph edges. Empty consumer groups are omitted on the wire. */
export interface CompositionDependencyIndex {
  variable_consumers?: Record<string, {
    input_component_ids?: string[];
    text_component_ids?: string[];
    query_variable_ids?: string[];
    metric_component_ids?: string[];
    table_component_ids?: string[];
  }>;
}

export interface CompositionSaveResponse {
  composition_id: string;
  revision_id: string;
  canonical_yaml: string;
  receipt: CompositionPublicationReceipt;
}

export interface CompositionRevisionBase {
  entryId: string;
  revisionId: string;
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

const supportedSqlArgumentTypes = new Set([
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
    supportedSqlArgumentTypes.has(variable.type)
  );

/** Thin browser adapter over the portable Composition graph protocol. */
export const compositionApi = {
  async canonicalizeDocument(
    document: unknown,
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
        ...(opts?.variables === undefined ? {} : { variables: opts.variables }),
        ...(opts?.unset_variables === undefined
          ? {}
          : { unset_variables: opts.unset_variables }),
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

  /** Authorized planner metadata for an editable graph draft. */
  async previewGraph(
    spaceId: string,
    yaml: string,
    variables: Record<string, unknown>,
    base?: CompositionRevisionBase,
    signal?: AbortSignal,
  ): Promise<CompositionGraphPreviewResponse> {
    const snapshot = splitVariableSnapshot(variables);
    return await protocolFetch<CompositionGraphPreviewResponse>(
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

  /** Evaluate a graph once against an immutable, server-authorized snapshot. */
  async evaluate(
    spaceId: string,
    request: CompositionEvaluationRequest,
    signal?: AbortSignal,
  ): Promise<CompositionEvaluationResponse> {
    return await protocolFetch<CompositionEvaluationResponse>(
      "composition.evaluate",
      { space_id: spaceId },
      request,
      { signal },
    );
  },
};

export const compositionDisplayName = (value: unknown): string =>
  typeof value === "string" && value.trim() ? value.trim() : "Composition";
