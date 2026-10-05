import type {
  CompositionParameterType,
  CompositionResultType,
} from "./composition-api";
import type {
  EntryQueryCompositionFieldSchemaEntry,
  EntryQueryCompositionFilter,
  EntryQueryCompositionProjection,
  EntryQueryCompositionSort,
} from "./entry-query-composition";
import { compositionApi } from "./composition-api";
import type { CompositionDocumentCanonicalization } from "./ugoite-client";

/**
 * Browser Work state for Composition authoring.
 *
 * The draft is never an authority: it holds form input, stable draft IDs,
 * and source/component association only. Validation, canonicalization,
 * query compilation, authorization, and metric semantics stay in the
 * shared Rust/WASM contract; every save or preview passes through
 * `canonicalizeCompositionDocument` first.
 */

/** Human-facing tool name source for lists; never persisted as identity. */
export interface DraftSavedSqlSeed {
  entryId: string;
  revisionId: string;
  name: string;
  expectedResult: Array<{ name: string; type: CompositionResultType }>;
  variables: Record<string, { parameter: string }>;
}

export interface DraftEntryQuerySeed {
  formId: string;
  name: string;
  fieldSchema: EntryQueryCompositionFieldSchemaEntry[];
  query: {
    text?: { parameter: string } | unknown;
    filters: EntryQueryCompositionFilter[];
    sort: EntryQueryCompositionSort[];
    projection: EntryQueryCompositionProjection;
    pageLimit?: number;
  };
}

export type DraftSource =
  | (DraftSavedSqlSeed & { kind: "saved_sql"; draftId: string })
  | (DraftEntryQuerySeed & { kind: "entry_query"; draftId: string });

export type DraftMetricValueField =
  | { fieldId: number }
  | { column: string };

export type DraftDisplay =
  | {
    kind: "table";
    draftId: string;
    sourceDraftId: string;
    label?: string;
  }
  | {
    kind: "metric";
    draftId: string;
    sourceDraftId: string;
    label?: string;
    valueField: DraftMetricValueField;
  };

export interface DraftParameter {
  id: string;
  type: CompositionParameterType;
  label?: string;
  required: boolean;
  default?: unknown;
  format?: "year-month";
}

export interface CompositionDraft {
  name: string;
  tags: string[];
  sources: DraftSource[];
  displays: DraftDisplay[];
  parameters: DraftParameter[];
  nextSourceSeq: number;
  nextDisplaySeq: number;
}

export type DraftErrorCode =
  | "unknown-source"
  | "unknown-display"
  | "unknown-parameter"
  | "duplicate-parameter"
  | "source-referenced"
  | "parameter-referenced";

export type DraftResult =
  | { ok: true; draft: CompositionDraft }
  | { ok: false; error: DraftErrorCode };

/** A typed Composition document assembled from a draft for canonicalization. */
export interface CompositionStudioDocument {
  format: "ugoite.composition";
  format_version: 1;
  kind: "dashboard";
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
    sources: Array<
      | {
        kind: "saved_sql";
        id: string;
        entry_id: string;
        revision_id: string;
        expected_result: Array<{ name: string; type: CompositionResultType }>;
        variables: Record<string, { parameter: string }>;
      }
      | {
        kind: "entry_query";
        id: string;
        form_id: string;
        field_schema: EntryQueryCompositionFieldSchemaEntry[];
        query: {
          text?: unknown;
          filters: EntryQueryCompositionFilter[];
          sort: EntryQueryCompositionSort[];
          page_limit?: number;
          projection: EntryQueryCompositionProjection;
        };
      }
    >;
    components: Array<
      | { kind: "table"; id: string; label?: string; source: string }
      | {
        kind: "metric";
        id: string;
        label?: string;
        source: string;
        value_field:
          | { kind: "entry_field"; field_id: number }
          | { kind: "sql_column"; name: string };
      }
    >;
    sections: Array<{ id: string; components: string[] }>;
  };
}

export const createEmptyDraft = (name = ""): CompositionDraft => ({
  name,
  tags: [],
  sources: [],
  displays: [],
  parameters: [],
  nextSourceSeq: 1,
  nextDisplaySeq: 1,
});

export const setDraftName = (
  draft: CompositionDraft,
  name: string,
): CompositionDraft => ({
  ...draft,
  name,
});

export const setDraftTags = (
  draft: CompositionDraft,
  tags: readonly string[],
): CompositionDraft => ({
  ...draft,
  tags: [...tags],
});

export const addSavedSqlSource = (
  draft: CompositionDraft,
  seed: DraftSavedSqlSeed,
): { ok: true; draft: CompositionDraft; draftId: string } => {
  const draftId = `src-${draft.nextSourceSeq}`;
  return {
    ok: true,
    draftId,
    draft: {
      ...draft,
      nextSourceSeq: draft.nextSourceSeq + 1,
      sources: [...draft.sources, { ...seed, kind: "saved_sql", draftId }],
    },
  };
};

export const addEntryQuerySource = (
  draft: CompositionDraft,
  seed: DraftEntryQuerySeed,
): { ok: true; draft: CompositionDraft; draftId: string } => {
  const draftId = `src-${draft.nextSourceSeq}`;
  return {
    ok: true,
    draftId,
    draft: {
      ...draft,
      nextSourceSeq: draft.nextSourceSeq + 1,
      sources: [...draft.sources, { ...seed, kind: "entry_query", draftId }],
    },
  };
};

/** Displays currently bound to one source; removing requires zero. */
export const displaysUsingSource = (
  draft: CompositionDraft,
  sourceDraftId: string,
): DraftDisplay[] =>
  draft.displays.filter((display) => display.sourceDraftId === sourceDraftId);

/** Blocked remove: the Browser never cascades display deletion silently. */
export const removeSource = (
  draft: CompositionDraft,
  sourceDraftId: string,
): DraftResult => {
  if (!draft.sources.some((source) => source.draftId === sourceDraftId)) {
    return { ok: false, error: "unknown-source" };
  }
  if (displaysUsingSource(draft, sourceDraftId).length > 0) {
    return { ok: false, error: "source-referenced" };
  }
  return {
    ok: true,
    draft: {
      ...draft,
      sources: draft.sources.filter(
        (source) => source.draftId !== sourceDraftId,
      ),
    },
  };
};

const moveId = (
  ids: readonly string[],
  id: string,
  direction: -1 | 1,
): readonly string[] | undefined => {
  const index = ids.indexOf(id);
  const target = index + direction;
  if (index < 0 || target < 0 || target >= ids.length) return undefined;
  const next = [...ids];
  [next[index], next[target]] = [next[target], next[index]];
  return next;
};

export const moveSource = (
  draft: CompositionDraft,
  sourceDraftId: string,
  direction: "up" | "down",
): DraftResult => {
  const order = moveId(
    draft.sources.map((source) => source.draftId),
    sourceDraftId,
    direction === "up" ? -1 : 1,
  );
  if (!order) return { ok: false, error: "unknown-source" };
  const byId = new Map(draft.sources.map((source) => [source.draftId, source]));
  return {
    ok: true,
    draft: {
      ...draft,
      sources: order.map((id) => byId.get(id)).filter((
        source,
      ): source is DraftSource => source !== undefined),
    },
  };
};

export const addTableDisplay = (
  draft: CompositionDraft,
  sourceDraftId: string,
  label?: string,
): DraftResult & { draftId?: string } => {
  if (!draft.sources.some((source) => source.draftId === sourceDraftId)) {
    return { ok: false, error: "unknown-source" };
  }
  const draftId = `disp-${draft.nextDisplaySeq}`;
  return {
    ok: true,
    draftId,
    draft: {
      ...draft,
      nextDisplaySeq: draft.nextDisplaySeq + 1,
      displays: [
        ...draft.displays,
        { kind: "table", draftId, sourceDraftId, ...(label ? { label } : {}) },
      ],
    },
  };
};

export const addMetricDisplay = (
  draft: CompositionDraft,
  sourceDraftId: string,
  valueField: DraftMetricValueField,
  label?: string,
): DraftResult & { draftId?: string } => {
  if (!draft.sources.some((source) => source.draftId === sourceDraftId)) {
    return { ok: false, error: "unknown-source" };
  }
  const draftId = `disp-${draft.nextDisplaySeq}`;
  return {
    ok: true,
    draftId,
    draft: {
      ...draft,
      nextDisplaySeq: draft.nextDisplaySeq + 1,
      displays: [
        ...draft.displays,
        {
          kind: "metric",
          draftId,
          sourceDraftId,
          valueField,
          ...(label ? { label } : {}),
        },
      ],
    },
  };
};

export const removeDisplay = (
  draft: CompositionDraft,
  displayDraftId: string,
): DraftResult => {
  if (!draft.displays.some((display) => display.draftId === displayDraftId)) {
    return { ok: false, error: "unknown-display" };
  }
  return {
    ok: true,
    draft: {
      ...draft,
      displays: draft.displays.filter(
        (display) => display.draftId !== displayDraftId,
      ),
    },
  };
};

export const moveDisplay = (
  draft: CompositionDraft,
  displayDraftId: string,
  direction: "up" | "down",
): DraftResult => {
  const order = moveId(
    draft.displays.map((display) => display.draftId),
    displayDraftId,
    direction === "up" ? -1 : 1,
  );
  if (!order) return { ok: false, error: "unknown-display" };
  const byId = new Map(
    draft.displays.map((display) => [display.draftId, display]),
  );
  return {
    ok: true,
    draft: {
      ...draft,
      displays: order.map((id) => byId.get(id)).filter((
        display,
      ): display is DraftDisplay => display !== undefined),
    },
  };
};

/** Rename a display label. A blank label clears back to the default name. */
export const setDisplayLabel = (
  draft: CompositionDraft,
  displayDraftId: string,
  label: string,
): DraftResult => {
  const index = draft.displays.findIndex((display) =>
    display.draftId === displayDraftId
  );
  if (index < 0) return { ok: false, error: "unknown-display" };
  const trimmed = label.trim();
  const current = draft.displays[index];
  const next = { ...current };
  if (trimmed) {
    next.label = trimmed;
  } else {
    delete next.label;
  }
  const displays = [...draft.displays];
  displays[index] = next;
  return { ok: true, draft: { ...draft, displays } };
};

export const upsertParameter = (
  draft: CompositionDraft,
  parameter: DraftParameter,
): DraftResult => {
  const existing = draft.parameters.findIndex((item) =>
    item.id === parameter.id
  );
  if (existing < 0) {
    return {
      ok: true,
      draft: { ...draft, parameters: [...draft.parameters, parameter] },
    };
  }
  const parameters = [...draft.parameters];
  parameters[existing] = parameter;
  return { ok: true, draft: { ...draft, parameters } };
};

export const addParameter = (
  draft: CompositionDraft,
  parameter: DraftParameter,
): DraftResult =>
  draft.parameters.some((item) => item.id === parameter.id)
    ? { ok: false, error: "duplicate-parameter" }
    : upsertParameter(draft, parameter);

/** Parameters referenced by sources or filters cannot be removed silently. */
export const removeParameter = (
  draft: CompositionDraft,
  parameterId: string,
): DraftResult => {
  if (!draft.parameters.some((item) => item.id === parameterId)) {
    return { ok: false, error: "unknown-parameter" };
  }
  const referenced = draft.sources.some((source) => {
    if (source.kind === "saved_sql") {
      return Object.values(source.variables).some((binding) =>
        binding.parameter === parameterId
      );
    }
    const mentions = (value: unknown): boolean => {
      if (typeof value !== "object" || value === null) return false;
      if ("parameter" in value) {
        return (value as { parameter: unknown }).parameter === parameterId;
      }
      return false;
    };
    if (mentions(source.query.text)) return true;
    return source.query.filters.some((filter) => mentions(filter.value));
  });
  if (referenced) return { ok: false, error: "parameter-referenced" };
  return {
    ok: true,
    draft: {
      ...draft,
      parameters: draft.parameters.filter((item) => item.id !== parameterId),
    },
  };
};

/**
 * Assemble the typed document for canonicalization. The Studio keeps one
 * `main` section in display order; named grouping stays out of the MVP.
 */
export const toStudioDocument = (
  draft: CompositionDraft,
): CompositionStudioDocument => ({
  format: "ugoite.composition",
  format_version: 1,
  kind: "dashboard",
  name: draft.name.trim(),
  tags: [...draft.tags],
  spec: {
    parameters: draft.parameters.map((parameter) => ({
      id: parameter.id,
      ...(parameter.label ? { label: parameter.label } : {}),
      type: parameter.type,
      required: parameter.required,
      ...(parameter.default === undefined
        ? {}
        : { default: parameter.default }),
      ...(parameter.format ? { format: parameter.format } : {}),
    })),
    sources: draft.sources.map((source) => {
      if (source.kind === "saved_sql") {
        return {
          kind: "saved_sql" as const,
          id: source.draftId,
          entry_id: source.entryId,
          revision_id: source.revisionId,
          expected_result: source.expectedResult.map((column) => ({
            ...column,
          })),
          variables: Object.fromEntries(
            Object.entries(source.variables).map((
              [name, binding],
            ) => [name, { ...binding }]),
          ),
        };
      }
      return {
        kind: "entry_query" as const,
        id: source.draftId,
        form_id: source.formId,
        field_schema: source.fieldSchema.map((entry) => ({ ...entry })),
        query: {
          ...(source.query.text === undefined
            ? {}
            : { text: source.query.text }),
          filters: source.query.filters.map((filter) => ({ ...filter })),
          sort: source.query.sort.map((clause) => ({ ...clause })),
          ...(source.query.pageLimit === undefined
            ? {}
            : { page_limit: source.query.pageLimit }),
          projection: source.query.projection,
        },
      };
    }),
    components: draft.displays.map((display) => {
      if (display.kind === "table") {
        return {
          kind: "table" as const,
          id: display.draftId,
          ...(display.label ? { label: display.label } : {}),
          source: display.sourceDraftId,
        };
      }
      return {
        kind: "metric" as const,
        id: display.draftId,
        ...(display.label ? { label: display.label } : {}),
        source: display.sourceDraftId,
        value_field: "fieldId" in display.valueField
          ? {
            kind: "entry_field" as const,
            field_id: display.valueField.fieldId,
          }
          : { kind: "sql_column" as const, name: display.valueField.column },
      };
    }),
    sections: [{
      id: "main",
      components: draft.displays.map((display) => display.draftId),
    }],
  },
});

/** Canonicalize the draft through the shared Rust/WASM contract. */
export const canonicalizeDraft = async (
  draft: CompositionDraft,
): Promise<CompositionDocumentCanonicalization> =>
  await compositionApi.canonicalizeDocument(toStudioDocument(draft));
