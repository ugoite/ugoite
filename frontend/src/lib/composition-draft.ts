import type {
  CompositionResultType,
  CompositionTextStyle,
  CompositionVariableType,
} from "./composition-api";
import { compositionApi } from "./composition-api";
import type { CompositionDocumentCanonicalization } from "./ugoite-client";
import { t } from "./i18n";
import { normalizeSqlVariables } from "./sql";

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
  sql: string;
  expectedResult: Array<{ name: string; type: CompositionResultType }>;
  /** SHA-256 of the exact revision's expected result schema, when available. */
  schemaFingerprint?: string;
  variables: Record<string, { variable: string }>;
  /** Declared variable types from this exact Saved SQL revision. */
  variableTypes?: Record<string, CompositionVariableType>;
  /** Optional defaults copied from the selected immutable SQL revision. */
  variableDefaults?: Record<string, unknown>;
}

export interface DraftInlineSqlSeed {
  name: string;
  sql: string;
  formBindings?: Array<{ relation: string; form_id: string }>;
  expectedResult?: Array<{ name: string; type: CompositionResultType }>;
  schemaFingerprint?: string;
}

export type DraftSource =
  | (DraftSavedSqlSeed & { kind: "saved_sql"; draftId: string })
  | (DraftInlineSqlSeed & { kind: "inline_sql"; draftId: string });

/**
 * Studio seed carried through router location state from an entry point
 * (Saved SQL run or Form picker) to the Studio new route. Reuses the
 * draft seed shapes only; no new source semantics.
 */
export type CompositionStudioSeed =
  | { kind: "saved_sql"; seed: DraftSavedSqlSeed }
  | { kind: "inline_sql"; seed: DraftInlineSqlSeed };

export interface StudioSeedState {
  seed: CompositionStudioSeed;
}

/** Fail-closed seed read: anything without a kind-tagged draft seed is ignored. */
export const studioSeedState = (
  value: unknown,
): StudioSeedState | undefined => {
  if (!value || typeof value !== "object") return undefined;
  const seed = (value as { seed?: unknown }).seed;
  if (!seed || typeof seed !== "object") return undefined;
  const tagged = seed as { kind?: unknown; seed?: unknown };
  if (tagged.kind !== "saved_sql" && tagged.kind !== "inline_sql") {
    return undefined;
  }
  if (!tagged.seed || typeof tagged.seed !== "object") return undefined;
  return { seed: seed as CompositionStudioSeed };
};

/**
 * Apply one Studio seed to a draft: add the source and add a default
 * Table on the new source, and prefill the tool name from the seed source
 * name so the Studio opens with a visible block and is save-ready
 * immediately. Every seed path emits at least one layout item: the default
 * Table placement never leaves a zero-display draft.
 */
export const applyStudioSeed = (
  draft: CompositionDraft,
  seed: CompositionStudioSeed,
): { draft: CompositionDraft; draftId: string } => {
  const added = seed.kind === "saved_sql"
    ? addSavedSqlSource(draft, seed.seed)
    : addInlineSqlSource(draft, seed.seed);
  let next = added.draft;
  // Fail-closed table placement keeps the seed one-shot and total: the
  // seeded source always exists, so this succeeds and the draft is never
  // zero-display. A rejection would keep the unplaced source only. The
  const tabled = addTableDisplay(next, added.draftId);
  if (tabled.ok) next = tabled.draft;
  const named = next.name.trim() || !seed.seed.name
    ? next
    : setDraftName(next, seed.seed.name);
  return { draft: named, draftId: added.draftId };
};

export interface DraftMetricPresentation {
  kind: "number";
  unit?: string;
  fractionDigits?: number;
}

export type DraftTextContent =
  | { kind: "literal"; text: string }
  | { kind: "variable"; variable: string };

export type DraftDisplay =
  | {
    kind: "table";
    draftId: string;
    sourceDraftId: string;
  }
  | {
    kind: "metric";
    draftId: string;
    sourceDraftId: string;
    column?: string;
    presentation?: DraftMetricPresentation;
  }
  | {
    kind: "text";
    draftId: string;
    content: DraftTextContent;
    style: CompositionTextStyle;
  };

/**
 * One placed Dashboard flow row. Every item references an ordinary
 * component draft ID; pixel positions and widths are never stored.
 * Cross-row item moves stay out of the draft: items reorder within their
 * row and rows reorder against each other.
 */
export type DraftLayoutItem = { kind: "component"; draftId: string };

export interface DraftLayoutRow {
  id: string;
  items: DraftLayoutItem[];
}

/**
 * Canvas insertion target. A null rowId opens a new row at rowIndex;
 * otherwise the item lands in the named row at itemIndex (clamped).
 */
export interface DraftInsertTarget {
  rowId: string | null;
  rowIndex: number;
  itemIndex: number;
}

export interface DraftVariable {
  id: string;
  type: CompositionVariableType;
  required: boolean;
  default?: unknown;
  format?: "year-month";
}

export interface DraftInput {
  draftId: string;
  variable: string;
}

const savedSqlVariableTypeSet = new Set<CompositionVariableType>([
  "string",
  "boolean",
  "integer",
  "float",
  "date",
  "timestamp",
]);

/** Read only the typed declarations captured by one exact Saved SQL revision. */
export const savedSqlVariableTypesFromFields = (
  fields: Record<string, unknown> | undefined,
): Record<string, CompositionVariableType> => {
  const raw = fields?.variables;
  if (raw === undefined || raw === null) return {};
  if (!Array.isArray(raw)) throw new Error("Invalid Saved SQL variables");
  const result: Record<string, CompositionVariableType> = {};
  for (const item of raw) {
    if (!item || typeof item !== "object") {
      throw new Error("Invalid Saved SQL variable");
    }
    const { name, type } = item as { name?: unknown; type?: unknown };
    if (
      typeof name !== "string" || !name.trim() || Object.hasOwn(result, name) ||
      typeof type !== "string" ||
      !savedSqlVariableTypeSet.has(type as CompositionVariableType)
    ) {
      throw new Error("Invalid Saved SQL variable");
    }
    Object.defineProperty(result, name, {
      value: type as CompositionVariableType,
      enumerable: true,
      configurable: true,
      writable: true,
    });
  }
  return result;
};

export interface CompositionDraft {
  name: string;
  sources: DraftSource[];
  displays: DraftDisplay[];
  variables: DraftVariable[];
  inputs: DraftInput[];
  /** First-class flow rows; document layout renders from these in order. */
  layoutRows: DraftLayoutRow[];
  nextSourceSeq: number;
  nextDisplaySeq: number;
  nextRowSeq: number;
}

export type DraftErrorCode =
  | "unknown-source"
  | "unknown-display"
  | "unknown-variable"
  | "unknown-input"
  | "unknown-row"
  | "unknown-block"
  | "invalid-style"
  | "invalid-query"
  | "duplicate-variable"
  | "variable-referenced"
  | "source-referenced";

export type DraftResult =
  | { ok: true; draft: CompositionDraft }
  | { ok: false; error: DraftErrorCode };

/** A typed Composition document assembled from a draft for canonicalization. */
export interface CompositionStudioDocument {
  format: "ugoite.composition";
  format_version: 1;
  kind: "dashboard";
  name: string;
  spec: {
    variables: Array<{
      id: string;
      type: CompositionVariableType;
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
        schema_fingerprint?: string;
        variables: Record<string, { variable: string }>;
      }
      | {
        kind: "inline_sql";
        id: string;
        name: string;
        sql: string;
        form_bindings: Array<{ relation: string; form_id: string }>;
        expected_result: Array<{ name: string; type: CompositionResultType }>;
        schema_fingerprint: string;
      }
    >;
    components: Array<
      | { kind: "input"; id: string; variable: string }
      | { kind: "table"; id: string; source: string }
      | {
        kind: "metric";
        id: string;
        source: string;
        column: string;
        presentation?: {
          kind: "number";
          unit?: string;
          fraction_digits?: number;
        };
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
    layout: {
      kind: "flow";
      rows: Array<{
        id: string;
        items: Array<{ kind: "component"; component: string }>;
      }>;
    };
  };
}

type CompositionStudioSavedSqlDocument = Extract<
  CompositionStudioDocument["spec"]["sources"][number],
  { kind: "saved_sql" }
>;

type CompositionStudioInlineSqlDocument = Extract<
  CompositionStudioDocument["spec"]["sources"][number],
  { kind: "inline_sql" }
>;

type CompositionStudioRevisionSourceDocument =
  | Omit<CompositionStudioSavedSqlDocument, "variables"> & {
    variables?: CompositionStudioSavedSqlDocument["variables"];
  }
  | CompositionStudioInlineSqlDocument;

/**
 * Composition read back from the canonical revision. The domain omits empty
 * optional collections, so the editor accepts missing variable and Saved SQL
 * variable collections and restores them as empty draft state.
 */
export type CompositionStudioRevisionDocument =
  & Omit<CompositionStudioDocument, "spec">
  & {
    spec: Omit<CompositionStudioDocument["spec"], "variables" | "sources"> & {
      variables?: CompositionStudioDocument["spec"]["variables"];
      sources: CompositionStudioRevisionSourceDocument[];
    };
  };

export const createEmptyDraft = (name = ""): CompositionDraft => ({
  name,
  sources: [],
  displays: [],
  variables: [],
  inputs: [],
  layoutRows: [{ id: "main", items: [] }],
  nextSourceSeq: 1,
  nextDisplaySeq: 1,
  nextRowSeq: 1,
});

/**
 * Build the isolated source/input portion used to preview one Saved SQL
 * Metric. Only variables bound by this source are relevant to canonicalizing
 * its query; retaining their placed Input components keeps required variable
 * validation intact without allowing unrelated draft edits to block preview.
 */
export const createMetricPreviewDraft = (
  snapshot: CompositionDraft,
  sourceDraftId: string,
): CompositionDraft | null => {
  const source = snapshot.sources.find((entry) =>
    entry.draftId === sourceDraftId
  );
  if (!source) return null;

  const variableIds = new Set(source.kind === "saved_sql"
    ? Object.values(source.variables).map((binding) => binding.variable)
    : normalizeSqlVariables(source.sql).variables.map((variable) =>
      variable.name
    ));
  const inputs = snapshot.inputs.filter((input) =>
    variableIds.has(input.variable)
  ).map((input) => ({ ...input }));
  const inputIds = new Set(inputs.map((input) => input.draftId));
  const layoutRows = snapshot.layoutRows.flatMap((row) => {
    const items = row.items.filter((item) => inputIds.has(item.draftId)).map(
      (item) => ({ ...item }),
    );
    return items.length > 0 ? [{ id: row.id, items }] : [];
  });

  return {
    ...createEmptyDraft("Metric preview"),
    sources: [{ ...source, variables: { ...source.variables } }],
    variables: snapshot.variables.filter((variable) =>
      variableIds.has(variable.id)
    ).map((variable) => ({ ...variable })),
    inputs,
    layoutRows: layoutRows.length > 0
      ? layoutRows
      : [{ id: "main", items: [] }],
    nextSourceSeq: snapshot.nextSourceSeq,
  };
};

export const setDraftName = (
  draft: CompositionDraft,
  name: string,
): CompositionDraft => ({
  ...draft,
  name,
});

export const addSavedSqlSource = (
  draft: CompositionDraft,
  seed: DraftSavedSqlSeed,
): { ok: true; draft: CompositionDraft; draftId: string } => {
  const draftId = `src-${draft.nextSourceSeq}`;
  const sourceVariables: Record<string, { variable: string }> = {
    ...seed.variables,
  };
  let nextDraft: CompositionDraft = {
    ...draft,
    nextSourceSeq: draft.nextSourceSeq + 1,
    sources: [...draft.sources, {
      ...seed,
      kind: "saved_sql",
      draftId,
      variables: sourceVariables,
      variableTypes: { ...(seed.variableTypes ?? {}) },
      variableDefaults: { ...(seed.variableDefaults ?? {}) },
    }],
  };
  for (const [sqlName, type] of Object.entries(seed.variableTypes ?? {})) {
    if (sourceVariables[sqlName]) continue;
    let variableId = sqlName;
    let suffix = 2;
    while (nextDraft.variables.some((variable) => variable.id === variableId)) {
      const existing = nextDraft.variables.find((variable) =>
        variable.id === variableId
      );
      if (existing?.type === type) break;
      variableId = `${sqlName}_${suffix++}`;
    }
    let variable = nextDraft.variables.find((entry) =>
      entry.id === variableId
    );
    if (!variable) {
      const defaultValue = seed.variableDefaults?.[sqlName];
      variable = {
        id: variableId,
        type,
        required: defaultValue === undefined,
        ...(defaultValue === undefined ? {} : { default: defaultValue }),
      };
      nextDraft = {
        ...nextDraft,
        variables: [...nextDraft.variables, variable],
      };
      if (defaultValue === undefined) {
        const input = addInputComponent(nextDraft, variableId);
        if (input.ok) nextDraft = input.draft;
      }
    }
    sourceVariables[sqlName] = { variable: variableId };
  }
  const sources = [...nextDraft.sources];
  const inserted = sources.findIndex((source) => source.draftId === draftId);
  const added = sources[inserted];
  if (added?.kind === "saved_sql") {
    sources[inserted] = { ...added, variables: sourceVariables };
  }
  return { ok: true, draftId, draft: { ...nextDraft, sources } };
};

export const addInlineSqlSource = (
  draft: CompositionDraft,
  seed: DraftInlineSqlSeed,
): { ok: true; draft: CompositionDraft; draftId: string } => {
  const draftId = `src-${draft.nextSourceSeq}`;
  return {
    ok: true,
    draftId,
    draft: {
      ...draft,
      nextSourceSeq: draft.nextSourceSeq + 1,
      sources: [...draft.sources, {
        ...seed,
        formBindings: seed.formBindings ?? [],
        expectedResult: seed.expectedResult ?? [],
        schemaFingerprint: seed.schemaFingerprint ?? "",
        kind: "inline_sql",
        draftId,
      }],
    },
  };
};

/** Displays currently bound to one source; removing requires zero. */
export const displaysUsingSource = (
  draft: CompositionDraft,
  sourceDraftId: string,
): DraftDisplay[] =>
  draft.displays.filter((display) =>
    "sourceDraftId" in display && display.sourceDraftId === sourceDraftId
  );

/** Layout rows with all items still referencing a known block. */
const pruneLayoutRows = (draft: CompositionDraft): DraftLayoutRow[] => {
  const known = new Set([
    ...draft.displays.map((display) => display.draftId),
    ...draft.inputs.map((input) => input.draftId),
  ]);
  return draft.layoutRows
    .map((row) => ({
      ...row,
      items: row.items.filter((item) => known.has(item.draftId)),
    }))
    .filter((row) => row.items.length > 0);
};

const nextRowId = (
  draft: CompositionDraft,
): { draft: CompositionDraft; rowId: string } => {
  let seq = draft.nextRowSeq;
  let rowId = `row-${seq}`;
  const taken = new Set(draft.layoutRows.map((row) => row.id));
  while (taken.has(rowId)) {
    seq += 1;
    rowId = `row-${seq}`;
  }
  return { draft: { ...draft, nextRowSeq: seq + 1 }, rowId };
};

/**
 * Place one layout item at a canvas insertion target. A null rowId opens
 * a new row at rowIndex; otherwise the item lands in the named row.
 */
const insertLayoutItem = (
  draft: CompositionDraft,
  item: DraftLayoutItem,
  target?: DraftInsertTarget,
): DraftResult => {
  if (!target) {
    if (draft.layoutRows.length === 0) {
      const named = nextRowId(draft);
      return {
        ok: true,
        draft: {
          ...named.draft,
          layoutRows: [{ id: named.rowId, items: [item] }],
        },
      };
    }
    const rows = draft.layoutRows.map((row) => ({
      ...row,
      items: [...row.items],
    }));
    rows[rows.length - 1].items.push(item);
    return { ok: true, draft: { ...draft, layoutRows: rows } };
  }
  if (target.rowId === null) {
    const named = nextRowId(draft);
    const rows = draft.layoutRows.map((row) => ({
      ...row,
      items: [...row.items],
    }));
    const rowIndex = Math.max(0, Math.min(target.rowIndex, rows.length));
    rows.splice(rowIndex, 0, { id: named.rowId, items: [item] });
    return { ok: true, draft: { ...named.draft, layoutRows: rows } };
  }
  const rowIndex = draft.layoutRows.findIndex((row) => row.id === target.rowId);
  if (rowIndex < 0) return { ok: false, error: "unknown-row" };
  const rows = draft.layoutRows.map((row) => ({
    ...row,
    items: [...row.items],
  }));
  const at = Math.max(
    0,
    Math.min(target.itemIndex, rows[rowIndex].items.length),
  );
  rows[rowIndex].items.splice(at, 0, item);
  return { ok: true, draft: { ...draft, layoutRows: rows } };
};

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

/** Update editable SQL text without rewriting it from structured controls. */
export const setInlineSql = (
  draft: CompositionDraft,
  sourceDraftId: string,
  sql: string,
): DraftResult => {
  const index = draft.sources.findIndex((source) =>
    source.draftId === sourceDraftId
  );
  if (index < 0) return { ok: false, error: "unknown-source" };
  const current = draft.sources[index];
  if (current.kind !== "inline_sql") return { ok: false, error: "unknown-source" };
  if (!sql.trim()) return { ok: false, error: "invalid-query" };
  const sources = [...draft.sources];
  sources[index] = {
    ...current,
    sql,
    formBindings: [],
    expectedResult: [],
    schemaFingerprint: "",
  };
  return { ok: true, draft: { ...draft, sources } };
};

export const setInlineSqlName = (
  draft: CompositionDraft,
  sourceDraftId: string,
  name: string,
): DraftResult => {
  const index = draft.sources.findIndex((source) =>
    source.draftId === sourceDraftId
  );
  if (index < 0) return { ok: false, error: "unknown-source" };
  const current = draft.sources[index];
  if (current.kind !== "inline_sql") {
    return { ok: false, error: "unknown-source" };
  }
  const sources = [...draft.sources];
  sources[index] = { ...current, name };
  return { ok: true, draft: { ...draft, sources } };
};

export interface InlineSqlDescriptorUpdate {
  formBindings: Array<{ relation: string; form_id: string }>;
  expectedResult: Array<{ name: string; type: CompositionResultType }>;
  schemaFingerprint: string;
}

/** Pin only the descriptor returned by the current authorized SQL preview. */
export const setInlineSqlDescriptor = (
  draft: CompositionDraft,
  sourceDraftId: string,
  descriptor: InlineSqlDescriptorUpdate,
): DraftResult => {
  const index = draft.sources.findIndex((source) =>
    source.draftId === sourceDraftId
  );
  if (index < 0) return { ok: false, error: "unknown-source" };
  const current = draft.sources[index];
  if (current.kind !== "inline_sql") {
    return { ok: false, error: "unknown-source" };
  }
  if (
    JSON.stringify(current.formBindings ?? []) ===
      JSON.stringify(descriptor.formBindings) &&
    JSON.stringify(current.expectedResult ?? []) ===
      JSON.stringify(descriptor.expectedResult) &&
    current.schemaFingerprint === descriptor.schemaFingerprint
  ) return { ok: true, draft };
  const sources = [...draft.sources];
  sources[index] = {
    ...current,
    formBindings: descriptor.formBindings.map((binding) => ({ ...binding })),
    expectedResult: descriptor.expectedResult.map((column) => ({ ...column })),
    schemaFingerprint: descriptor.schemaFingerprint,
  };
  return { ok: true, draft: { ...draft, sources } };
};

export interface SavedSqlRevisionUpdate {
  revisionId: string;
  expectedResult: Array<{ name: string; type: CompositionResultType }>;
  schemaFingerprint?: string;
  /** Variable types declared by this exact revision. */
  variableTypes: Record<string, CompositionVariableType>;
}

/**
 * Point a Saved SQL source at an exact new revision. Retain only existing
 * bindings whose declared variable still exists with the same type. Never
 * guesses a same-name Composition variable or falls back to latest.
 */
export const setSavedSqlRevision = (
  draft: CompositionDraft,
  sourceDraftId: string,
  revision: SavedSqlRevisionUpdate,
): DraftResult => {
  const index = draft.sources.findIndex((source) =>
    source.draftId === sourceDraftId
  );
  if (index < 0) return { ok: false, error: "unknown-source" };
  const current = draft.sources[index];
  if (current.kind !== "saved_sql") {
    return { ok: false, error: "unknown-source" };
  }
  if (!revision.revisionId.trim()) return { ok: false, error: "invalid-query" };
  for (const column of revision.expectedResult) {
    if (!column.name.trim() || !column.type) {
      return { ok: false, error: "invalid-query" };
    }
  }
  const variables: Record<string, { variable: string }> = {};
  for (const name of Object.keys(revision.variableTypes)) {
    if (!name.trim()) return { ok: false, error: "invalid-query" };
    if (!Object.hasOwn(current.variables, name)) continue;
    const existing = current.variables[name];
    if (!existing) continue;
    const variable = draft.variables.find((item) =>
      item.id === existing.variable
    );
    if (variable?.type === revision.variableTypes[name]) {
      Object.defineProperty(variables, name, {
        value: { ...existing },
        enumerable: true,
        configurable: true,
        writable: true,
      });
    }
  }
  const sources = [...draft.sources];
  sources[index] = {
    ...current,
    revisionId: revision.revisionId,
    expectedResult: revision.expectedResult.map((column) => ({ ...column })),
    // A missing value intentionally clears the prior revision's fingerprint;
    // canonicalizeDraft derives the hash from this revision's exact schema.
    schemaFingerprint: revision.schemaFingerprint,
    variables,
    variableTypes: { ...revision.variableTypes },
  };
  return { ok: true, draft: { ...draft, sources } };
};

/** Record the pinned revision's type schema and remove stale bindings. */
export const setSavedSqlVariableTypes = (
  draft: CompositionDraft,
  sourceDraftId: string,
  variableTypes: Record<string, CompositionVariableType>,
): DraftResult => {
  const index = draft.sources.findIndex((source) =>
    source.draftId === sourceDraftId
  );
  if (index < 0 || draft.sources[index].kind !== "saved_sql") {
    return { ok: false, error: "unknown-source" };
  }
  const source = draft.sources[index] as Extract<
    DraftSource,
    { kind: "saved_sql" }
  >;
  const variables = Object.fromEntries(
    Object.entries(source.variables).filter(([name, binding]) => {
      const definition = draft.variables.find((item) =>
        item.id === binding.variable
      );
      return !!definition && definition.type === variableTypes[name];
    }),
  );
  const sources = [...draft.sources];
  sources[index] = {
    ...source,
    variableTypes: { ...variableTypes },
    variables,
  };
  return { ok: true, draft: { ...draft, sources } };
};

/** Bind one SQL variable to an exact-type Composition variable, or clear it. */
export const setSavedSqlVariableBinding = (
  draft: CompositionDraft,
  sourceDraftId: string,
  sqlVariableName: string,
  variableId: string | undefined,
): DraftResult => {
  const index = draft.sources.findIndex((source) =>
    source.draftId === sourceDraftId
  );
  if (index < 0 || draft.sources[index].kind !== "saved_sql") {
    return { ok: false, error: "unknown-source" };
  }
  const source = draft.sources[index] as Extract<
    DraftSource,
    { kind: "saved_sql" }
  >;
  const variableTypes = source.variableTypes ?? {};
  if (
    !sqlVariableName.trim() || !Object.hasOwn(variableTypes, sqlVariableName)
  ) {
    return { ok: false, error: "invalid-query" };
  }
  const sqlType = variableTypes[sqlVariableName];
  const variables = { ...source.variables };
  if (variableId === undefined) {
    delete variables[sqlVariableName];
  } else {
    const variable = draft.variables.find((item) => item.id === variableId);
    if (!variable) return { ok: false, error: "unknown-variable" };
    if (variable.type !== sqlType) return { ok: false, error: "invalid-query" };
    Object.defineProperty(variables, sqlVariableName, {
      value: { variable: variableId },
      enumerable: true,
      configurable: true,
      writable: true,
    });
  }
  const sources = [...draft.sources];
  sources[index] = { ...source, variables };
  return { ok: true, draft: { ...draft, sources } };
};

export const addTableDisplay = (
  draft: CompositionDraft,
  sourceDraftId: string,
  target?: DraftInsertTarget,
): DraftResult & { draftId?: string } => {
  if (!draft.sources.some((source) => source.draftId === sourceDraftId)) {
    return { ok: false, error: "unknown-source" };
  }
  const displayIdentity = nextDisplayDraftIdentity(draft);
  const draftId = displayIdentity.draftId;
  const display: DraftDisplay = {
    kind: "table",
    draftId,
    sourceDraftId,
  };
  const placed = insertLayoutItem(
    {
      ...draft,
      nextDisplaySeq: displayIdentity.nextDisplaySeq,
      displays: [...draft.displays, display],
    },
    { kind: "component", draftId },
    target,
  );
  if (!placed.ok) return placed;
  return { ok: true, draftId, draft: placed.draft };
};

const metricColumnFitsSource = (
  source: DraftSource,
  columnName: string | undefined,
  requireReady: boolean,
): boolean => {
  if (!columnName?.trim()) return !requireReady;
  return (source.expectedResult ?? []).filter((column) =>
    column.name === columnName && column.type !== "json"
  ).length === 1;
};

export const metricColumnReady = (
  draft: CompositionDraft,
  sourceDraftId: string,
  columnName: string | undefined,
): boolean => {
  const source = draft.sources.find((entry) => entry.draftId === sourceDraftId);
  return source !== undefined &&
    metricColumnFitsSource(source, columnName, true);
};

export const draftMetricsReady = (draft: CompositionDraft): boolean =>
  draft.displays.every((display) =>
    display.kind !== "metric" ||
    metricColumnReady(draft, display.sourceDraftId, display.column)
  );

const numericMetricResultTypes = new Set([
  "integer",
  "float",
  "double",
  "decimal",
]);

export const metricColumnIsNumeric = (
  source: DraftSource,
  columnName: string | undefined,
): boolean => {
  if (!columnName) return false;
  const columns = (source.expectedResult ?? []).filter((column) =>
    column.name === columnName && column.type !== "json"
  );
  return columns.length === 1 && numericMetricResultTypes.has(columns[0].type);
};

export const addMetricDisplay = (
  draft: CompositionDraft,
  sourceDraftId: string,
  column: string,
  target?: DraftInsertTarget,
): DraftResult & { draftId?: string } => {
  const source = draft.sources.find((entry) => entry.draftId === sourceDraftId);
  if (!source) return { ok: false, error: "unknown-source" };
  if (!metricColumnFitsSource(source, column, true)) {
    return { ok: false, error: "invalid-query" };
  }
  const displayIdentity = nextDisplayDraftIdentity(draft);
  const draftId = displayIdentity.draftId;
  const display: DraftDisplay = {
    kind: "metric",
    draftId,
    sourceDraftId,
    column,
  };
  const placed = insertLayoutItem(
    {
      ...draft,
      nextDisplaySeq: displayIdentity.nextDisplaySeq,
      displays: [...draft.displays, display],
    },
    { kind: "component", draftId },
    target,
  );
  if (!placed.ok) return placed;
  return { ok: true, draftId, draft: placed.draft };
};

/** Declare a fixed-style text block and place it exactly once in layout. */
export const addTextDisplay = (
  draft: CompositionDraft,
  init: { text?: string; style?: CompositionTextStyle } = {},
  target?: DraftInsertTarget,
): DraftResult & { draftId?: string } => {
  const displayIdentity = nextDisplayDraftIdentity(draft);
  const draftId = displayIdentity.draftId;
  const display: DraftDisplay = {
    kind: "text",
    draftId,
    content: { kind: "literal", text: init.text ?? "" },
    style: init.style ?? "body",
  };
  const placed = insertLayoutItem(
    {
      ...draft,
      nextDisplaySeq: displayIdentity.nextDisplaySeq,
      displays: [...draft.displays, display],
    },
    { kind: "component", draftId },
    target,
  );
  if (!placed.ok) return placed;
  return { ok: true, draftId, draft: placed.draft };
};

const nextDisplayDraftIdentity = (
  draft: CompositionDraft,
): { draftId: string; nextDisplaySeq: number } => {
  const usedIds = new Set([
    ...draft.displays.map((display) => display.draftId),
    ...draft.inputs.map((input) => input.draftId),
  ]);
  let seq = draft.nextDisplaySeq;
  while (usedIds.has(`disp-${seq}`)) seq += 1;
  return { draftId: `disp-${seq}`, nextDisplaySeq: seq + 1 };
};

/** Add an ordinary Input component for a declared variable. */
export const addInputComponent = (
  draft: CompositionDraft,
  variableId: string,
  target?: DraftInsertTarget,
): DraftResult & { draftId?: string } => {
  if (!draft.variables.some((variable) => variable.id === variableId)) {
    return { ok: false, error: "unknown-variable" };
  }
  const ids = new Set([
    ...draft.displays.map((display) => display.draftId),
    ...draft.inputs.map((input) => input.draftId),
  ]);
  let sequence = draft.nextDisplaySeq;
  let draftId = `input-${sequence}`;
  while (ids.has(draftId)) {
    sequence += 1;
    draftId = `input-${sequence}`;
  }
  const input: DraftInput = { draftId, variable: variableId };
  const placed = insertLayoutItem(
    {
      ...draft,
      nextDisplaySeq: sequence + 1,
      inputs: [...draft.inputs, input],
    },
    { kind: "component", draftId },
    target,
  );
  return placed.ok ? { ...placed, draftId } : placed;
};

/** Edit the variable binding of an existing Input component. */
export const setInputVariable = (
  draft: CompositionDraft,
  inputDraftId: string,
  variableId: string,
): DraftResult => {
  if (!draft.variables.some((variable) => variable.id === variableId)) {
    return { ok: false, error: "unknown-variable" };
  }
  const index = draft.inputs.findIndex((input) =>
    input.draftId === inputDraftId
  );
  if (index < 0) return { ok: false, error: "unknown-input" };
  const inputs = [...draft.inputs];
  inputs[index] = { ...inputs[index], variable: variableId };
  return { ok: true, draft: { ...draft, inputs } };
};

/** Remove one Input component and its placement; the variable remains. */
export const removeInputComponent = (
  draft: CompositionDraft,
  inputDraftId: string,
): DraftResult => {
  if (!draft.inputs.some((input) => input.draftId === inputDraftId)) {
    return { ok: false, error: "unknown-input" };
  }
  const next = {
    ...draft,
    inputs: draft.inputs.filter((input) => input.draftId !== inputDraftId),
    layoutRows: draft.layoutRows
      .map((row) => ({
        ...row,
        items: row.items.filter((item) => item.draftId !== inputDraftId),
      }))
      .filter((row) => row.items.length > 0),
  };
  return { ok: true, draft: next };
};

/** Reorder layout rows; the row order is the document render order. */
export const moveLayoutRow = (
  draft: CompositionDraft,
  rowId: string,
  direction: "up" | "down",
): DraftResult => {
  const order = moveId(
    draft.layoutRows.map((row) => row.id),
    rowId,
    direction === "up" ? -1 : 1,
  );
  if (!order) return { ok: false, error: "unknown-row" };
  const byId = new Map(draft.layoutRows.map((row) => [row.id, row]));
  return {
    ok: true,
    draft: {
      ...draft,
      layoutRows: order.map((id) => byId.get(id)).filter((
        row,
      ): row is DraftLayoutRow => row !== undefined),
    },
  };
};

/**
 * Reorder one item within its row. Cross-row moves stay unsupported:
 * placement across rows goes through insertion targets instead.
 */
export const moveLayoutItem = (
  draft: CompositionDraft,
  rowId: string,
  itemIndex: number,
  direction: "up" | "down",
): DraftResult => {
  const rowIndex = draft.layoutRows.findIndex((row) => row.id === rowId);
  if (rowIndex < 0) return { ok: false, error: "unknown-row" };
  const row = draft.layoutRows[rowIndex];
  const target = itemIndex + (direction === "up" ? -1 : 1);
  if (
    itemIndex < 0 || itemIndex >= row.items.length || target < 0 ||
    target >= row.items.length
  ) {
    return { ok: false, error: "unknown-block" };
  }
  const rows = draft.layoutRows.map((entry) => ({
    ...entry,
    items: [...entry.items],
  }));
  [rows[rowIndex].items[itemIndex], rows[rowIndex].items[target]] = [
    rows[rowIndex].items[target],
    rows[rowIndex].items[itemIndex],
  ];
  return { ok: true, draft: { ...draft, layoutRows: rows } };
};

export const removeDisplay = (
  draft: CompositionDraft,
  displayDraftId: string,
): DraftResult => {
  if (!draft.displays.some((display) => display.draftId === displayDraftId)) {
    return { ok: false, error: "unknown-display" };
  }
  const next: CompositionDraft = {
    ...draft,
    displays: draft.displays.filter(
      (display) => display.draftId !== displayDraftId,
    ),
  };
  // Removing the declaration unplaces it; rows left empty are dropped.
  next.layoutRows = pruneLayoutRows(next);
  return { ok: true, draft: next };
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
  // The flat legacy list and the 2D layout stay in sync: same-row
  // neighbors swap in place, cross-row neighbors exchange positions so
  // every component keeps exactly one placement.
  const locate = (id: string): { row: number; index: number } | undefined => {
    for (let row = 0; row < draft.layoutRows.length; row += 1) {
      const index = draft.layoutRows[row].items.findIndex((item) =>
        item.kind === "component" && item.draftId === id
      );
      if (index >= 0) return { row, index };
    }
    return undefined;
  };
  const from = locate(displayDraftId);
  const at = draft.displays.findIndex((display) =>
    display.draftId === displayDraftId
  );
  const neighborId = draft.displays[at + (direction === "up" ? -1 : 1)]
    ?.draftId;
  const to = neighborId ? locate(neighborId) : undefined;
  const layoutRows = draft.layoutRows.map((row) => ({
    ...row,
    items: [...row.items],
  }));
  if (from && to) {
    const moved = layoutRows[from.row].items[from.index];
    layoutRows[from.row].items[from.index] = layoutRows[to.row].items[to.index];
    layoutRows[to.row].items[to.index] = moved;
  }
  return {
    ok: true,
    draft: {
      ...draft,
      displays: order.map((id) => byId.get(id)).filter((
        display,
      ): display is DraftDisplay => display !== undefined),
      layoutRows,
    },
  };
};

/** Retarget a Metric to a Saved SQL source; a column must be selected again. */
export const setMetricSource = (
  draft: CompositionDraft,
  displayDraftId: string,
  sourceDraftId: string,
): DraftResult => {
  const index = draft.displays.findIndex((display) =>
    display.draftId === displayDraftId
  );
  if (index < 0) return { ok: false, error: "unknown-display" };
  const current = draft.displays[index];
  if (current.kind !== "metric") return { ok: false, error: "unknown-display" };
  const source = draft.sources.find((entry) => entry.draftId === sourceDraftId);
  if (!source) return { ok: false, error: "unknown-source" };
  const next = { ...current, sourceDraftId };
  delete next.column;
  delete next.presentation;
  const displays = [...draft.displays];
  displays[index] = next;
  return { ok: true, draft: { ...draft, displays } };
};

/** Retarget a table to another existing source; the label is untouched. */
export const setTableSource = (
  draft: CompositionDraft,
  displayDraftId: string,
  sourceDraftId: string,
): DraftResult => {
  const index = draft.displays.findIndex((display) =>
    display.draftId === displayDraftId
  );
  if (index < 0) return { ok: false, error: "unknown-display" };
  const current = draft.displays[index];
  if (current.kind !== "table") return { ok: false, error: "unknown-display" };
  if (!draft.sources.some((source) => source.draftId === sourceDraftId)) {
    return { ok: false, error: "unknown-source" };
  }
  const displays = [...draft.displays];
  displays[index] = { ...current, sourceDraftId };
  return { ok: true, draft: { ...draft, displays } };
};

/** Select one displayable output column from the Metric's Saved SQL source. */
export const setMetricColumn = (
  draft: CompositionDraft,
  displayDraftId: string,
  column: string | undefined,
): DraftResult => {
  const index = draft.displays.findIndex((display) =>
    display.draftId === displayDraftId
  );
  if (index < 0) return { ok: false, error: "unknown-display" };
  const current = draft.displays[index];
  if (current.kind !== "metric") return { ok: false, error: "unknown-display" };
  const source = draft.sources.find((entry) =>
    entry.draftId === current.sourceDraftId
  );
  if (!source) return { ok: false, error: "unknown-source" };
  if (!metricColumnFitsSource(source, column, false)) {
    return { ok: false, error: "invalid-query" };
  }
  const next = { ...current };
  if (column === undefined || column === "") delete next.column;
  else next.column = column;
  if (!metricColumnIsNumeric(source, column)) {
    delete next.presentation;
  }
  const displays = [...draft.displays];
  displays[index] = next;
  return { ok: true, draft: { ...draft, displays } };
};

/** Edit optional numeric display hints without changing Metric query identity. */
export const setMetricPresentation = (
  draft: CompositionDraft,
  displayDraftId: string,
  presentation: DraftMetricPresentation,
): DraftResult => {
  const index = draft.displays.findIndex((display) =>
    display.draftId === displayDraftId
  );
  if (index < 0) return { ok: false, error: "unknown-display" };
  const current = draft.displays[index];
  if (current.kind !== "metric") return { ok: false, error: "unknown-display" };
  const source = draft.sources.find((entry) =>
    entry.draftId === current.sourceDraftId
  );
  if (!source) return { ok: false, error: "unknown-source" };
  if (!metricColumnIsNumeric(source, current.column)) {
    return { ok: false, error: "invalid-query" };
  }
  const unit = presentation.unit;
  const hasUnit = unit?.trim();
  const fractionDigits = presentation.fractionDigits;
  if (
    fractionDigits !== undefined &&
    (!Number.isInteger(fractionDigits) || fractionDigits < 0 ||
      fractionDigits > 255)
  ) return { ok: false, error: "invalid-query" };

  const next = { ...current };
  if (!hasUnit && fractionDigits === undefined) {
    delete next.presentation;
  } else {
    next.presentation = {
      kind: "number",
      ...(hasUnit && unit !== undefined ? { unit } : {}),
      ...(fractionDigits === undefined ? {} : { fractionDigits }),
    };
  }
  const displays = [...draft.displays];
  displays[index] = next;
  return { ok: true, draft: { ...draft, displays } };
};

/** Edit text content; style, sources, and layout stay untouched. */
export const setTextContent = (
  draft: CompositionDraft,
  displayDraftId: string,
  text: string,
): DraftResult => {
  const index = draft.displays.findIndex((display) =>
    display.draftId === displayDraftId
  );
  if (index < 0) return { ok: false, error: "unknown-display" };
  const current = draft.displays[index];
  if (current.kind !== "text") return { ok: false, error: "unknown-display" };
  const displays = [...draft.displays];
  displays[index] = { ...current, content: { kind: "literal", text } };
  return { ok: true, draft: { ...draft, displays } };
};

/** Bind a Text component directly to one string variable. */
export const setTextVariable = (
  draft: CompositionDraft,
  displayDraftId: string,
  variableId: string,
): DraftResult => {
  const index = draft.displays.findIndex((display) =>
    display.draftId === displayDraftId
  );
  if (index < 0) return { ok: false, error: "unknown-display" };
  const current = draft.displays[index];
  if (current.kind !== "text") return { ok: false, error: "unknown-display" };
  const variable = draft.variables.find((item) => item.id === variableId);
  if (!variable) return { ok: false, error: "unknown-variable" };
  if (variable.type !== "string") return { ok: false, error: "invalid-query" };
  const displays = [...draft.displays];
  displays[index] = {
    ...current,
    content: { kind: "variable", variable: variableId },
  };
  return { ok: true, draft: { ...draft, displays } };
};

const textStyles: ReadonlySet<CompositionTextStyle> = new Set([
  "title",
  "heading",
  "body",
  "caption",
]);

/** Edit a text style; only the fixed enum is accepted. */
export const setTextStyle = (
  draft: CompositionDraft,
  displayDraftId: string,
  style: CompositionTextStyle,
): DraftResult => {
  const index = draft.displays.findIndex((display) =>
    display.draftId === displayDraftId
  );
  if (index < 0) return { ok: false, error: "unknown-display" };
  const current = draft.displays[index];
  if (current.kind !== "text") return { ok: false, error: "unknown-display" };
  if (!textStyles.has(style)) return { ok: false, error: "invalid-style" };
  const displays = [...draft.displays];
  displays[index] = { ...current, style };
  return { ok: true, draft: { ...draft, displays } };
};

export const upsertVariable = (
  draft: CompositionDraft,
  variable: DraftVariable,
): DraftResult => {
  const existing = draft.variables.findIndex((item) => item.id === variable.id);
  if (existing < 0) {
    return {
      ok: true,
      draft: { ...draft, variables: [...draft.variables, variable] },
    };
  }
  const variables = [...draft.variables];
  variables[existing] = variable;
  return { ok: true, draft: { ...draft, variables } };
};

export const addVariable = (
  draft: CompositionDraft,
  variable: DraftVariable,
): DraftResult =>
  draft.variables.some((item) => item.id === variable.id)
    ? { ok: false, error: "duplicate-variable" }
    : upsertVariable(draft, variable);

/** Initial runtime values from explicit variable defaults; Inputs stay Work. */
export const defaultVariableValues = (
  draft: CompositionDraft,
): Record<string, unknown> => {
  const values: Record<string, unknown> = {};
  for (const variable of draft.variables) {
    if (variable.default !== undefined) {
      values[variable.id] = variable.default;
    }
  }
  return values;
};

/** Variables referenced by sources, Inputs, or Text cannot be removed silently. */
export const removeVariable = (
  draft: CompositionDraft,
  variableId: string,
): DraftResult => {
  if (!draft.variables.some((item) => item.id === variableId)) {
    return { ok: false, error: "unknown-variable" };
  }
  if (draft.inputs.some((input) => input.variable === variableId)) {
    return { ok: false, error: "variable-referenced" };
  }
  if (
    draft.displays.some((display) =>
      display.kind === "text" && display.content.kind === "variable" &&
      display.content.variable === variableId
    )
  ) return { ok: false, error: "variable-referenced" };
  const referenced = draft.sources.some((source) => {
    if (source.kind === "saved_sql") {
      return Object.values(source.variables).some((binding) =>
        binding.variable === variableId
      );
    }
    if (source.kind === "inline_sql") {
      return normalizeSqlVariables(source.sql).variables.some((variable) =>
        variable.name === variableId
      );
    }
    return false;
  });
  if (referenced) return { ok: false, error: "variable-referenced" };
  return {
    ok: true,
    draft: {
      ...draft,
      variables: draft.variables.filter((item) => item.id !== variableId),
    },
  };
};

/**
 * Assemble the typed document for canonicalization. Components render
 * from the first-class layout rows in order; text carries no source.
 * An empty draft yields an empty row, which the shared contract rejects
 * fail-closed at canonicalization.
 */
export const toStudioDocument = (
  draft: CompositionDraft,
): CompositionStudioDocument => ({
  format: "ugoite.composition",
  format_version: 1,
  kind: "dashboard",
  name: draft.name.trim(),
  spec: {
    variables: draft.variables.map((variable) => ({
      id: variable.id,
      type: variable.type,
      required: variable.required,
      ...(variable.default === undefined ? {} : { default: variable.default }),
      ...(variable.format ? { format: variable.format } : {}),
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
          ...(source.schemaFingerprint
            ? { schema_fingerprint: source.schemaFingerprint }
            : {}),
          variables: Object.fromEntries(
            Object.entries(source.variables).map(([name, binding]) => [
              name,
              { variable: binding.variable },
            ]),
          ),
        };
      }
      return {
        kind: "inline_sql" as const,
        id: source.draftId,
        name: source.name,
        sql: source.sql,
        form_bindings: source.formBindings ?? [],
        expected_result: source.expectedResult ?? [],
        schema_fingerprint: source.schemaFingerprint ?? "",
      };
    }),
    components: [
      ...draft.displays.map((display) => {
        if (display.kind === "table") {
          return {
            kind: "table" as const,
            id: display.draftId,
            source: display.sourceDraftId,
          };
        }
        if (display.kind === "text") {
          return {
            kind: "text" as const,
            id: display.draftId,
            content: { ...display.content },
            style: display.style,
          };
        }
        return {
          kind: "metric" as const,
          id: display.draftId,
          source: display.sourceDraftId,
          column: display.column ?? "",
          ...(display.presentation
            ? {
              presentation: {
                kind: "number" as const,
                ...(display.presentation.unit === undefined
                  ? {}
                  : { unit: display.presentation.unit }),
                ...(display.presentation.fractionDigits === undefined
                  ? {}
                  : { fraction_digits: display.presentation.fractionDigits }),
              },
            }
            : {}),
        };
      }),
      ...draft.inputs.map((input) => ({
        kind: "input" as const,
        id: input.draftId,
        variable: input.variable,
      })),
    ],
    layout: {
      kind: "flow",
      // Empty rows carry no placement meaning and the domain rejects them,
      // so they never reach the document; palette insertions routinely leave
      // the initial empty row behind. A draft with no placed items at all
      // still emits one empty row so the failure stays a clean validation
      // diagnostic instead of a shape error.
      rows: (() => {
        const placed = draft.layoutRows.filter((row) => row.items.length > 0);
        const rows = (placed.length > 0 ? placed : [{ id: "main", items: [] }])
          .map((row) => ({
            id: row.id,
            items: row.items.map((item) => ({
              kind: "component" as const,
              component: item.draftId,
            })),
          }));
        return rows;
      })(),
    },
  },
});

/**
 * Rebuild an editable draft from a server-normalized Composition document.
 * Structural mapping only: lint already normalized, so no validation logic
 * lives here. Unknown source or component kinds throw
 * fail-closed instead of approximating. Human names come from `sourceNames`
 * (keyed by document source id); unavailable names use the source kind label.
 */
export const draftFromDocument = (
  document: CompositionStudioRevisionDocument,
  sourceNames: Record<string, string>,
): CompositionDraft => {
  const fail = (what: string): never => {
    throw new Error(`Unsupported Composition document ${what}`);
  };
  if (
    document.spec.variables !== undefined &&
    !Array.isArray(document.spec.variables)
  ) {
    return fail("variables collection");
  }
  const variables = document.spec.variables ?? [];
  const sources: DraftSource[] = document.spec.sources.map((source, index) => {
    const draftId = `src-${index + 1}`;
    const name = sourceNames[source.id]?.trim() ||
      (source.kind === "saved_sql"
        ? t("spaceShell.title.savedSql")
        : t("common.form"));
    if (source.kind === "saved_sql") {
      return {
        kind: "saved_sql",
        draftId,
        entryId: source.entry_id,
        revisionId: source.revision_id,
        name,
        expectedResult: source.expected_result.map((column) => ({ ...column })),
        ...(source.schema_fingerprint
          ? { schemaFingerprint: source.schema_fingerprint }
          : {}),
        variables: Object.fromEntries(
          Object.entries(
            source.variables === undefined ? {} : source.variables,
          ).map(([key, binding]) => [key, { variable: binding.variable }]),
        ),
      };
    }
    if (source.kind === "inline_sql") {
      return {
        kind: "inline_sql",
        draftId,
        name: source.name,
        sql: source.sql,
        formBindings: source.form_bindings.map((binding) => ({ ...binding })),
        expectedResult: source.expected_result.map((column) => ({ ...column })),
        schemaFingerprint: source.schema_fingerprint,
      };
    }
    return fail(`source kind: ${String((source as { kind: unknown }).kind)}`);
  });
  const sourceDraftIds = new Map(
    document.spec.sources.map((
      source,
      index,
    ) => [source.id, `src-${index + 1}`]),
  );
  const inputByComponentId = new Map<
    string,
    Extract<CompositionStudioDocument["spec"]["components"][number], {
      kind: "input";
    }>
  >();
  for (const component of document.spec.components) {
    if (component.kind !== "input") continue;
    inputByComponentId.set(component.id, component);
  }
  const inputs = [...inputByComponentId.values()].map((input) => ({
    draftId: input.id,
    variable: input.variable,
  }));
  const displayComponents = document.spec.components.filter((component) =>
    component.kind !== "input"
  );
  const inputComponentIds = new Set(inputByComponentId.keys());
  const componentDraftIds = new Map<string, string>();
  let nextDisplaySeq = 1;
  for (const component of displayComponents) {
    while (inputComponentIds.has(`disp-${nextDisplaySeq}`)) {
      nextDisplaySeq += 1;
    }
    componentDraftIds.set(component.id, `disp-${nextDisplaySeq}`);
    nextDisplaySeq += 1;
  }
  const displays: DraftDisplay[] = displayComponents.map(
    (component) => {
      const draftId = componentDraftIds.get(component.id)!;
      if (component.kind === "text") {
        if (
          (component.style !== "title" && component.style !== "heading" &&
            component.style !== "body" && component.style !== "caption")
        ) {
          return fail(`text component shape: ${component.id}`);
        }
        let content: DraftTextContent;
        if (
          component.content.kind === "literal" &&
          typeof component.content.text === "string"
        ) {
          content = { kind: "literal", text: component.content.text };
        } else if (
          component.content.kind === "variable" &&
          typeof component.content.variable === "string" &&
          variables.some((variable) =>
            variable.id === component.content.variable
          )
        ) {
          content = {
            kind: "variable",
            variable: component.content.variable,
          };
        } else {
          return fail(`text component content: ${component.id}`);
        }
        return {
          kind: "text",
          draftId,
          content,
          style: component.style,
        };
      }
      const componentSourceId = component.source;
      const sourceDraftId = sourceDraftIds.get(componentSourceId);
      if (sourceDraftId === undefined) {
        return fail(`component source: ${componentSourceId}`);
      }
      if (component.kind === "table") {
        return {
          kind: "table",
          draftId,
          sourceDraftId,
        };
      }
      if (component.kind === "metric") {
        const draftSource = sources.find((entry) =>
          entry.draftId === sourceDraftId
        );
        if (!draftSource) return fail("metric source draft is missing");
        if (!metricColumnFitsSource(draftSource, component.column, true)) {
          return fail("metric column is invalid for its source");
        }
        return {
          kind: "metric",
          draftId,
          sourceDraftId,
          column: component.column,
          ...(component.presentation
            ? {
              presentation: {
                kind: "number",
                ...(component.presentation.unit === undefined
                  ? {}
                  : { unit: component.presentation.unit }),
                ...(component.presentation.fraction_digits === undefined
                  ? {}
                  : { fractionDigits: component.presentation.fraction_digits }),
              },
            }
            : {}),
        };
      }
      return fail(
        `component kind: ${String((component as { kind: unknown }).kind)}`,
      );
    },
  );
  const variableIds = new Set(variables.map((variable) => variable.id));
  const layoutRows: DraftLayoutRow[] = document.spec.layout.rows.map((row) => ({
    id: row.id,
    items: row.items.map((item) => {
      if (item.kind === "component") {
        const draftId = componentDraftIds.get(item.component);
        if (draftId) return { kind: "component" as const, draftId };
        const input = inputByComponentId.get(item.component);
        if (input && variableIds.has(input.variable)) {
          return { kind: "component" as const, draftId: input.id };
        }
        return fail(`layout component: ${item.component}`);
      }
      return fail(
        `layout item kind: ${String((item as { kind: unknown }).kind)}`,
      );
    }),
  }));
  return {
    name: document.name,
    sources,
    displays,
    variables: variables.map((variable) => ({
      id: variable.id,
      type: variable.type,
      required: variable.required,
      ...(variable.default === undefined ? {} : { default: variable.default }),
      ...(variable.format ? { format: variable.format } : {}),
    })),
    inputs,
    layoutRows,
    nextSourceSeq: sources.length + 1,
    nextDisplaySeq,
    nextRowSeq: layoutRows.length + 1,
  };
};

/** Save-blocking reason for a draft that is not save-ready. */
export type DraftSaveBlockedReason =
  | "name"
  | "sources"
  | "layout"
  | "refs"
  | "metric";

export interface DraftSaveReadiness {
  readonly ready: boolean;
  readonly reason?: DraftSaveBlockedReason;
}

/**
 * Browser save-readiness gate for Composition drafts. Mirrors the shared
 * canonical contract synchronously: a non-empty name, at least one source,
 * valid refs with every component placed exactly once and no dangling
 * refs, required variables without a default connected to an Input, complete
 * Metric definitions, and at least one layout item. A source-only draft
 * reports `layout` and cannot save. Canonical parsing itself stays async in
 * `canonicalizeDraft`; the save handler fails closed on its diagnostics.
 *
 * Deliberately unchecked here (and left to canonicalize): duplicate row
 * IDs and empty rows. Row IDs are system-generated with a taken-set, so no
 * draft operation can produce them; the direction is lenient, never
 * blocking a domain-valid draft.
 */
export const draftSaveReadiness = (
  draft: CompositionDraft,
): DraftSaveReadiness => {
  if (draft.name.trim().length === 0) return { ready: false, reason: "name" };
  if (draft.sources.length === 0) return { ready: false, reason: "sources" };
  const sourceIds = new Set(draft.sources.map((source) => source.draftId));
  const componentIds = new Set([
    ...draft.displays.map((display) => display.draftId),
    ...draft.inputs.map((input) => input.draftId),
  ]);
  const variableIds = new Set(
    draft.variables.map((variable) => variable.id),
  );
  for (const display of draft.displays) {
    if (display.kind === "text") {
      if (display.content.kind === "variable") {
        const variable = draft.variables.find((item) =>
          item.id === display.content.variable
        );
        if (variable?.type !== "string") {
          return { ready: false, reason: "refs" };
        }
      }
      continue;
    }
    if (!sourceIds.has(display.sourceDraftId)) {
      return { ready: false, reason: "refs" };
    }
  }
  for (const input of draft.inputs) {
    if (!variableIds.has(input.variable)) {
      return { ready: false, reason: "refs" };
    }
  }
  for (const source of draft.sources) {
    if (source.kind !== "saved_sql") continue;
    if (source.variableTypes === undefined) {
      return { ready: false, reason: "refs" };
    }
    for (const [sqlVariable, type] of Object.entries(source.variableTypes)) {
      const binding = source.variables[sqlVariable];
      const variable = binding &&
        draft.variables.find((item) => item.id === binding.variable);
      if (!variable || variable.type !== type) {
        return { ready: false, reason: "refs" };
      }
    }
  }
  if (!draftMetricsReady(draft)) return { ready: false, reason: "metric" };
  const placedComponents = new Map<string, number>();
  for (const row of draft.layoutRows) {
    for (const item of row.items) {
      if (!componentIds.has(item.draftId)) {
        return { ready: false, reason: "refs" };
      }
      placedComponents.set(
        item.draftId,
        (placedComponents.get(item.draftId) ?? 0) + 1,
      );
    }
  }
  for (const componentId of componentIds) {
    if (placedComponents.get(componentId) !== 1) {
      return { ready: false, reason: "refs" };
    }
  }
  for (const variable of draft.variables) {
    if (
      variable.required && variable.default === undefined &&
      !draft.inputs.some((input) => input.variable === variable.id)
    ) {
      return { ready: false, reason: "refs" };
    }
  }
  let items = 0;
  for (const row of draft.layoutRows) items += row.items.length;
  if (items === 0) return { ready: false, reason: "layout" };
  return { ready: true };
};

export const compositionResultSchemaFingerprint = async (
  columns: Array<{ name: string; type: CompositionResultType }>,
): Promise<string> => {
  const canonical = new TextEncoder().encode(JSON.stringify(columns));
  const digest = await crypto.subtle.digest("SHA-256", canonical);
  return Array.from(
    new Uint8Array(digest),
    (byte) => byte.toString(16).padStart(2, "0"),
  ).join("");
};

/** Canonicalize the draft through the shared Rust/WASM contract. */
export const canonicalizeDraft = async (
  draft: CompositionDraft,
): Promise<CompositionDocumentCanonicalization> => {
  const document = toStudioDocument(draft);
  const sources = await Promise.all(
    document.spec.sources.map(async (source) =>
      source.kind === "saved_sql"
        ? {
          ...source,
          schema_fingerprint: source.schema_fingerprint ||
            await compositionResultSchemaFingerprint(source.expected_result),
        }
        : source
    ),
  );
  return await compositionApi.canonicalizeDocument({
    ...document,
    spec: { ...document.spec, sources },
  });
};
