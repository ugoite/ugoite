import type {
  CompositionParameterType,
  CompositionResultType,
  CompositionTextStyle,
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
  /** Declared Saved SQL variable types for parameter provisioning. */
  variableTypes?: Record<string, CompositionParameterType>;
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

/**
 * Studio seed carried through router location state from an entry point
 * (Saved SQL run, EntryQuery browser) to the Studio new route. Reuses the
 * draft seed shapes only; no new source semantics.
 */
export type CompositionStudioSeed =
  | { kind: "saved_sql"; seed: DraftSavedSqlSeed }
  | { kind: "entry_query"; seed: DraftEntryQuerySeed };

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
  if (tagged.kind !== "saved_sql" && tagged.kind !== "entry_query") {
    return undefined;
  }
  if (!tagged.seed || typeof tagged.seed !== "object") return undefined;
  return { seed: seed as CompositionStudioSeed };
};

/**
 * Apply one Studio seed to a draft: add the source, provision Saved SQL
 * parameters from the server-declared variable types, and prefill the tool
 * name from the seed source name so the Studio is save-ready immediately.
 */
export const applyStudioSeed = (
  draft: CompositionDraft,
  seed: CompositionStudioSeed,
): { draft: CompositionDraft; draftId: string } => {
  const added = seed.kind === "saved_sql"
    ? addSavedSqlSource(draft, seed.seed)
    : addEntryQuerySource(draft, seed.seed);
  const provisioned = seed.kind === "saved_sql" && seed.seed.variableTypes
    ? ensureParametersForVariables(added.draft, seed.seed.variableTypes)
    : added.draft;
  const named = provisioned.name.trim() || !seed.seed.name
    ? provisioned
    : setDraftName(provisioned, seed.seed.name);
  return { draft: named, draftId: added.draftId };
};

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
  }
  | {
    kind: "text";
    draftId: string;
    text: string;
    style: CompositionTextStyle;
    label?: string;
  };

/**
 * One placed Dashboard flow row. Items reference component draft IDs or
 * semantic parameter IDs; pixel positions and widths are never stored.
 * Cross-row item moves stay out of the draft: items reorder within their
 * row and rows reorder against each other.
 */
export type DraftLayoutItem =
  | { kind: "component"; draftId: string }
  | { kind: "parameter"; parameterId: string };

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
  /** First-class flow rows; document layout renders from these in order. */
  layoutRows: DraftLayoutRow[];
  nextSourceSeq: number;
  nextDisplaySeq: number;
  nextRowSeq: number;
}

export type DraftErrorCode =
  | "unknown-source"
  | "unknown-display"
  | "unknown-parameter"
  | "unknown-row"
  | "unknown-block"
  | "invalid-style"
  | "duplicate-parameter"
  | "parameter-referenced"
  | "parameter-already-placed"
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
      | {
        kind: "text";
        id: string;
        label?: string;
        text: string;
        style: CompositionTextStyle;
      }
    >;
    layout: {
      kind: "flow";
      rows: Array<{
        id: string;
        items: Array<
          | { kind: "component"; component: string }
          | { kind: "parameter"; parameter: string }
        >;
      }>;
    };
  };
}

export const createEmptyDraft = (name = ""): CompositionDraft => ({
  name,
  tags: [],
  sources: [],
  displays: [],
  parameters: [],
  layoutRows: [{ id: "main", items: [] }],
  nextSourceSeq: 1,
  nextDisplaySeq: 1,
  nextRowSeq: 1,
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
  draft.displays.filter((display) =>
    "sourceDraftId" in display && display.sourceDraftId === sourceDraftId
  );

/** Semantic parameters with no layout control; the canvas offers these. */
export const unplacedParameters = (
  draft: CompositionDraft,
): DraftParameter[] => {
  const placed = new Set<string>();
  for (const row of draft.layoutRows) {
    for (const item of row.items) {
      if (item.kind === "parameter") placed.add(item.parameterId);
    }
  }
  return draft.parameters.filter((parameter) => !placed.has(parameter.id));
};

/** Layout rows with all items still referencing a known block. */
const pruneLayoutRows = (draft: CompositionDraft): DraftLayoutRow[] => {
  const known = new Set(draft.displays.map((display) => display.draftId));
  const parameters = new Set(draft.parameters.map((parameter) => parameter.id));
  return draft.layoutRows
    .map((row) => ({
      ...row,
      items: row.items.filter((item) =>
        item.kind === "component"
          ? known.has(item.draftId)
          : parameters.has(item.parameterId)
      ),
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

export const addTableDisplay = (
  draft: CompositionDraft,
  sourceDraftId: string,
  label?: string,
  target?: DraftInsertTarget,
): DraftResult & { draftId?: string } => {
  if (!draft.sources.some((source) => source.draftId === sourceDraftId)) {
    return { ok: false, error: "unknown-source" };
  }
  const draftId = `disp-${draft.nextDisplaySeq}`;
  const display: DraftDisplay = {
    kind: "table",
    draftId,
    sourceDraftId,
    ...(label ? { label } : {}),
  };
  const placed = insertLayoutItem(
    {
      ...draft,
      nextDisplaySeq: draft.nextDisplaySeq + 1,
      displays: [...draft.displays, display],
    },
    { kind: "component", draftId },
    target,
  );
  if (!placed.ok) return placed;
  return { ok: true, draftId, draft: placed.draft };
};

export const addMetricDisplay = (
  draft: CompositionDraft,
  sourceDraftId: string,
  valueField: DraftMetricValueField,
  label?: string,
  target?: DraftInsertTarget,
): DraftResult & { draftId?: string } => {
  if (!draft.sources.some((source) => source.draftId === sourceDraftId)) {
    return { ok: false, error: "unknown-source" };
  }
  const draftId = `disp-${draft.nextDisplaySeq}`;
  const display: DraftDisplay = {
    kind: "metric",
    draftId,
    sourceDraftId,
    valueField,
    ...(label ? { label } : {}),
  };
  const placed = insertLayoutItem(
    {
      ...draft,
      nextDisplaySeq: draft.nextDisplaySeq + 1,
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
  init: { text?: string; style?: CompositionTextStyle; label?: string } = {},
  target?: DraftInsertTarget,
): DraftResult & { draftId?: string } => {
  const draftId = `disp-${draft.nextDisplaySeq}`;
  const display: DraftDisplay = {
    kind: "text",
    draftId,
    text: init.text ?? "",
    style: init.style ?? "body",
    ...(init.label ? { label: init.label } : {}),
  };
  const placed = insertLayoutItem(
    {
      ...draft,
      nextDisplaySeq: draft.nextDisplaySeq + 1,
      displays: [...draft.displays, display],
    },
    { kind: "component", draftId },
    target,
  );
  if (!placed.ok) return placed;
  return { ok: true, draftId, draft: placed.draft };
};

/**
 * Place an existing semantic parameter as a layout control. Creating a
 * new parameter stays in the Parameters section; the canvas only places.
 */
export const placeParameterControl = (
  draft: CompositionDraft,
  parameterId: string,
  target?: DraftInsertTarget,
): DraftResult => {
  if (!draft.parameters.some((parameter) => parameter.id === parameterId)) {
    return { ok: false, error: "unknown-parameter" };
  }
  if (
    unplacedParameters(draft).every((parameter) => parameter.id !== parameterId)
  ) {
    return { ok: false, error: "parameter-already-placed" };
  }
  return insertLayoutItem(draft, { kind: "parameter", parameterId }, target);
};

/** Unplace a parameter control; the semantic parameter stays declared. */
export const unplaceParameterControl = (
  draft: CompositionDraft,
  parameterId: string,
): DraftResult => {
  if (!draft.parameters.some((parameter) => parameter.id === parameterId)) {
    return { ok: false, error: "unknown-parameter" };
  }
  const placed = draft.layoutRows.some((row) =>
    row.items.some((item) =>
      item.kind === "parameter" && item.parameterId === parameterId
    )
  );
  if (!placed) return { ok: false, error: "unknown-block" };
  const next: CompositionDraft = {
    ...draft,
    layoutRows: draft.layoutRows
      .map((row) => ({
        ...row,
        items: row.items.filter((item) =>
          item.kind !== "parameter" || item.parameterId !== parameterId
        ),
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

/**
 * Retarget a metric to another existing source with an explicit value
 * field. The caller keeps the current value field when it stays valid for
 * the new source and otherwise passes the new source's first scalar
 * candidate; the draft never infers column types.
 */
export const setMetricSource = (
  draft: CompositionDraft,
  displayDraftId: string,
  sourceDraftId: string,
  valueField: DraftMetricValueField,
): DraftResult => {
  const index = draft.displays.findIndex((display) =>
    display.draftId === displayDraftId
  );
  if (index < 0) return { ok: false, error: "unknown-display" };
  const current = draft.displays[index];
  if (current.kind !== "metric") return { ok: false, error: "unknown-display" };
  if (!draft.sources.some((source) => source.draftId === sourceDraftId)) {
    return { ok: false, error: "unknown-source" };
  }
  const displays = [...draft.displays];
  displays[index] = { ...current, sourceDraftId, valueField };
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

/** Rebind a metric value field; source and label stay as declared. */
export const setMetricValueField = (
  draft: CompositionDraft,
  displayDraftId: string,
  valueField: DraftMetricValueField,
): DraftResult => {
  const index = draft.displays.findIndex((display) =>
    display.draftId === displayDraftId
  );
  if (index < 0) return { ok: false, error: "unknown-display" };
  const current = draft.displays[index];
  if (current.kind !== "metric") return { ok: false, error: "unknown-display" };
  const displays = [...draft.displays];
  displays[index] = { ...current, valueField };
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
  displays[index] = { ...current, text };
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

/**
 * Move a placed parameter control to another semantic parameter in place.
 * The control keeps its row and position; both declarations stay owned by
 * the Parameters section, and no label override is created.
 */
export const retargetParameterControl = (
  draft: CompositionDraft,
  fromParameterId: string,
  toParameterId: string,
): DraftResult => {
  if (
    !draft.parameters.some((parameter) => parameter.id === fromParameterId)
  ) {
    return { ok: false, error: "unknown-parameter" };
  }
  if (!draft.parameters.some((parameter) => parameter.id === toParameterId)) {
    return { ok: false, error: "unknown-parameter" };
  }
  if (fromParameterId === toParameterId) return { ok: true, draft };
  let placed = false;
  for (const row of draft.layoutRows) {
    for (const item of row.items) {
      if (item.kind === "parameter" && item.parameterId === fromParameterId) {
        placed = true;
      }
      if (item.kind === "parameter" && item.parameterId === toParameterId) {
        return { ok: false, error: "parameter-already-placed" };
      }
    }
  }
  if (!placed) return { ok: false, error: "unknown-block" };
  return {
    ok: true,
    draft: {
      ...draft,
      layoutRows: draft.layoutRows.map((row) => ({
        ...row,
        items: row.items.map((item) =>
          item.kind === "parameter" && item.parameterId === fromParameterId
            ? { kind: "parameter" as const, parameterId: toParameterId }
            : item
        ),
      })),
    },
  };
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

/**
 * Provision required parameters for Saved SQL variable bindings. Existing
 * parameters are kept as-is (type mismatches surface as Rust-owned resolve
 * diagnostics, never silent rebinds); missing ones are added from the
 * server-declared variable types.
 */
export const ensureParametersForVariables = (
  draft: CompositionDraft,
  variableTypes: Readonly<Record<string, CompositionParameterType>>,
): CompositionDraft => {
  let next = draft;
  for (const [id, type] of Object.entries(variableTypes)) {
    if (next.parameters.some((item) => item.id === id)) continue;
    const added = addParameter(next, { id, type, required: true });
    if (added.ok) next = added.draft;
  }
  return next;
};

/** Caller parameter values carried by defaults; inputs stay Work. */
export const defaultParameterValues = (
  draft: CompositionDraft,
): Record<string, unknown> => {
  const values: Record<string, unknown> = {};
  for (const parameter of draft.parameters) {
    if (parameter.default !== undefined) {
      values[parameter.id] = parameter.default;
    }
  }
  return values;
};

/** Parameters referenced by sources, filters, or layout controls cannot be removed silently. */
export const removeParameter = (
  draft: CompositionDraft,
  parameterId: string,
): DraftResult => {
  if (!draft.parameters.some((item) => item.id === parameterId)) {
    return { ok: false, error: "unknown-parameter" };
  }
  const placed = draft.layoutRows.some((row) =>
    row.items.some((item) =>
      item.kind === "parameter" && item.parameterId === parameterId
    )
  );
  if (placed) return { ok: false, error: "parameter-referenced" };
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
      if (display.kind === "text") {
        return {
          kind: "text" as const,
          id: display.draftId,
          ...(display.label ? { label: display.label } : {}),
          text: display.text,
          style: display.style,
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
    layout: {
      kind: "flow",
      rows: draft.layoutRows.length > 0
        ? draft.layoutRows.map((row) => ({
          id: row.id,
          items: row.items.map((item) =>
            item.kind === "component"
              ? { kind: "component" as const, component: item.draftId }
              : { kind: "parameter" as const, parameter: item.parameterId }
          ),
        }))
        : [{ id: "main", items: [] }],
    },
  },
});

/**
 * Rebuild an editable draft from a server-normalized Composition document.
 * Structural mapping only: lint already normalized, so no validation logic
 * lives here. Unknown source, component, or value-field kinds throw
 * fail-closed instead of approximating. Human names come from `sourceNames`
 * (keyed by document source id) and fall back to the source id.
 */
export const draftFromDocument = (
  document: CompositionStudioDocument,
  sourceNames: Record<string, string>,
): CompositionDraft => {
  const fail = (what: string): never => {
    throw new Error(`Unsupported Composition document ${what}`);
  };
  const sources: DraftSource[] = document.spec.sources.map((source, index) => {
    const draftId = `src-${index + 1}`;
    const name = sourceNames[source.id] ?? source.id;
    if (source.kind === "saved_sql") {
      return {
        kind: "saved_sql",
        draftId,
        entryId: source.entry_id,
        revisionId: source.revision_id,
        name,
        expectedResult: source.expected_result.map((column) => ({ ...column })),
        variables: Object.fromEntries(
          Object.entries(source.variables).map(([key, binding]) => [
            key,
            { ...binding },
          ]),
        ),
      };
    }
    if (source.kind === "entry_query") {
      return {
        kind: "entry_query",
        draftId,
        formId: source.form_id,
        name,
        fieldSchema: source.field_schema.map((entry) => ({ ...entry })),
        query: {
          ...(source.query.text === undefined
            ? {}
            : { text: source.query.text }),
          filters: source.query.filters.map((filter) => ({ ...filter })),
          sort: source.query.sort.map((clause) => ({ ...clause })),
          ...(source.query.page_limit === undefined
            ? {}
            : { pageLimit: source.query.page_limit }),
          projection: source.query.projection,
        },
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
  const displays: DraftDisplay[] = document.spec.components.map(
    (component, index) => {
      const draftId = `disp-${index + 1}`;
      if (component.kind === "text") {
        if (
          typeof component.text !== "string" ||
          (component.style !== "title" && component.style !== "heading" &&
            component.style !== "body" && component.style !== "caption")
        ) {
          return fail(`text component shape: ${component.id}`);
        }
        return {
          kind: "text",
          draftId,
          text: component.text,
          style: component.style,
          ...(component.label ? { label: component.label } : {}),
        };
      }
      const sourceDraftId = sourceDraftIds.get(component.source);
      if (sourceDraftId === undefined) {
        return fail(`component source: ${component.source}`);
      }
      if (component.kind === "table") {
        return {
          kind: "table",
          draftId,
          sourceDraftId,
          ...(component.label ? { label: component.label } : {}),
        };
      }
      if (component.kind === "metric") {
        const valueField = component.value_field;
        if (valueField?.kind === "entry_field") {
          return {
            kind: "metric",
            draftId,
            sourceDraftId,
            ...(component.label ? { label: component.label } : {}),
            valueField: { fieldId: valueField.field_id },
          };
        }
        if (valueField?.kind === "sql_column") {
          return {
            kind: "metric",
            draftId,
            sourceDraftId,
            ...(component.label ? { label: component.label } : {}),
            valueField: { column: valueField.name },
          };
        }
        return fail(
          `metric value field kind: ${
            String(
              (valueField as { kind: unknown } | undefined)?.kind,
            )
          }`,
        );
      }
      return fail(
        `component kind: ${String((component as { kind: unknown }).kind)}`,
      );
    },
  );
  const componentDraftIds = new Map(
    document.spec.components.map((component, index) => [
      component.id,
      `disp-${index + 1}`,
    ]),
  );
  const parameterIds = new Set(
    document.spec.parameters.map((parameter) => parameter.id),
  );
  const layoutRows: DraftLayoutRow[] = document.spec.layout.rows.map((row) => ({
    id: row.id,
    items: row.items.map((item) => {
      if (item.kind === "component") {
        const draftId = componentDraftIds.get(item.component);
        if (!draftId) return fail(`layout component: ${item.component}`);
        return { kind: "component" as const, draftId };
      }
      if (item.kind === "parameter") {
        if (!parameterIds.has(item.parameter)) {
          return fail(`layout parameter: ${item.parameter}`);
        }
        return { kind: "parameter" as const, parameterId: item.parameter };
      }
      return fail(
        `layout item kind: ${String((item as { kind: unknown }).kind)}`,
      );
    }),
  }));
  return {
    name: document.name,
    tags: [...document.tags],
    sources,
    displays,
    parameters: document.spec.parameters.map((parameter) => ({
      id: parameter.id,
      ...(parameter.label ? { label: parameter.label } : {}),
      type: parameter.type,
      required: parameter.required,
      ...(parameter.default === undefined
        ? {}
        : { default: parameter.default }),
      ...(parameter.format ? { format: parameter.format } : {}),
    })),
    layoutRows,
    nextSourceSeq: sources.length + 1,
    nextDisplaySeq: displays.length + 1,
    nextRowSeq: layoutRows.length + 1,
  };
};

/** Canonicalize the draft through the shared Rust/WASM contract. */
export const canonicalizeDraft = async (
  draft: CompositionDraft,
): Promise<CompositionDocumentCanonicalization> =>
  await compositionApi.canonicalizeDocument(toStudioDocument(draft));
