import { describe, expect, it, vi } from "vitest";
import {
  addEntryQuerySource,
  addMetricDisplay,
  addParameter,
  addSavedSqlSource,
  addTableDisplay,
  addTextDisplay,
  canonicalizeDraft,
  type CompositionStudioDocument,
  createEmptyDraft,
  defaultParameterValues,
  displaysUsingSource,
  draftFromDocument,
  ensureParametersForVariables,
  moveDisplay,
  moveLayoutItem,
  moveLayoutRow,
  moveSource,
  placeParameterControl,
  removeDisplay,
  removeParameter,
  removeSource,
  setDraftName,
  setDraftTags,
  studioSeedState,
  toStudioDocument,
  unplacedParameters,
  unplaceParameterControl,
  upsertParameter,
} from "./composition-draft";
import { compositionApi } from "./composition-api";

vi.mock("./ugoite-client", () => ({
  canonicalizeCompositionDocument: vi.fn(async (document: unknown) => ({
    document,
    canonical_yaml: "canonical",
    fingerprint: "fp",
  })),
  entryApi: { query: vi.fn() },
  sqlApi: { query: vi.fn() },
  protocolFetch: vi.fn(),
  evaluateCompositionMetricPage: vi.fn(),
}));

const sqlSeed = () => ({
  entryId: "sql-1",
  revisionId: "rev-1",
  name: "Monthly totals",
  expectedResult: [{ name: "total", type: "float" as const }],
  variables: {},
});

const entrySeed = () => ({
  formId: "form-1",
  name: "Expenses",
  fieldSchema: [{ field_id: 1, field_type: "string" }],
  query: { filters: [], sort: [], projection: { kind: "preview" as const } },
});

describe("composition draft model", () => {
  it("builds stable draft identities without owning semantics", () => {
    let draft = createEmptyDraft("Monthly expenses");
    const first = addSavedSqlSource(draft, sqlSeed());
    expect(first.draftId).toBe("src-1");
    draft = first.draft;
    const second = addEntryQuerySource(draft, entrySeed());
    expect(second.draftId).toBe("src-2");
    draft = second.draft;

    const table = addTableDisplay(draft, "src-2", "Transactions");
    expect(table.draftId).toBe("disp-1");
    if (!table.ok) throw new Error("expected table display");
    const metric = addMetricDisplay(
      table.draft,
      "src-1",
      { column: "total" },
      "Total",
    );
    expect(metric.draftId).toBe("disp-2");
    if (!metric.ok) throw new Error("expected metric display");
    draft = metric.draft;

    const document = toStudioDocument(draft);
    expect(document.spec.sources.map((source) => source.id)).toEqual([
      "src-1",
      "src-2",
    ]);
    expect(document.spec.components.map((component) => component.id)).toEqual([
      "disp-1",
      "disp-2",
    ]);
    expect(document.spec.layout).toEqual({
      kind: "flow",
      rows: [{
        id: "main",
        items: [
          { kind: "component", component: "disp-1" },
          { kind: "component", component: "disp-2" },
        ],
      }],
    });
    expect(document.spec.components[1]).toMatchObject({
      kind: "metric",
      value_field: { kind: "sql_column", name: "total" },
    });
  });

  it("blocks source removal while displays reference it", () => {
    let draft = createEmptyDraft();
    draft = addSavedSqlSource(draft, sqlSeed()).draft;
    const added = addTableDisplay(draft, "src-1");
    if (!added.ok) throw new Error("expected display");
    draft = added.draft;

    expect(displaysUsingSource(draft, "src-1")).toHaveLength(1);
    expect(removeSource(draft, "src-1")).toEqual({
      ok: false,
      error: "source-referenced",
    });

    const removed = removeDisplay(draft, "disp-1");
    if (!removed.ok) throw new Error("expected display removal");
    const freed = removeSource(removed.draft, "src-1");
    expect(freed.ok).toBe(true);
    if (freed.ok) expect(freed.draft.sources).toHaveLength(0);
  });

  it("reorders sources and displays by keyboard-operable moves", () => {
    let draft = createEmptyDraft();
    draft = addSavedSqlSource(draft, sqlSeed()).draft;
    draft = addEntryQuerySource(draft, entrySeed()).draft;
    const moved = moveSource(draft, "src-1", "down");
    expect(moved.ok).toBe(true);
    if (moved.ok) {
      expect(moved.draft.sources.map((source) => source.draftId)).toEqual([
        "src-2",
        "src-1",
      ]);
    }
    expect(moveSource(draft, "src-1", "up")).toEqual({
      ok: false,
      error: "unknown-source",
    });
    expect(moveSource(draft, "nope", "down")).toEqual({
      ok: false,
      error: "unknown-source",
    });

    const one = addTableDisplay(draft, "src-1");
    if (!one.ok || !one.draftId) throw new Error("expected display");
    const second = addTableDisplay(one.draft, "src-2");
    if (!second.ok) throw new Error("expected display");
    draft = second.draft;
    const displayMoved = moveDisplay(draft, "disp-2", "up");
    expect(displayMoved.ok).toBe(true);
    if (displayMoved.ok) {
      expect(displayMoved.draft.displays.map((display) => display.draftId))
        .toEqual([
          "disp-2",
          "disp-1",
        ]);
      expect(toStudioDocument(displayMoved.draft).spec.layout.rows[0].items)
        .toEqual([
          { kind: "component", component: "disp-2" },
          { kind: "component", component: "disp-1" },
        ]);
    }
  });

  it("guards parameters against duplicates and silent unbinding", () => {
    let draft = createEmptyDraft();
    const parameter = { id: "month", type: "date" as const, required: true };
    const added = addParameter(draft, parameter);
    expect(added.ok).toBe(true);
    if (!added.ok) throw new Error("expected parameter");
    draft = added.draft;
    expect(addParameter(draft, parameter)).toEqual({
      ok: false,
      error: "duplicate-parameter",
    });
    expect(removeParameter(draft, "nope")).toEqual({
      ok: false,
      error: "unknown-parameter",
    });

    const updated = upsertParameter(draft, { ...parameter, required: false });
    expect(updated.ok).toBe(true);

    // A parameter bound by a source variable cannot be removed silently.
    const seeded = addSavedSqlSource(createEmptyDraft(), {
      ...sqlSeed(),
      variables: { month: { parameter: "month" } },
    }).draft;
    const withParam = addParameter(seeded, parameter);
    if (!withParam.ok) throw new Error("expected parameter");
    expect(removeParameter(withParam.draft, "month")).toEqual({
      ok: false,
      error: "parameter-referenced",
    });
  });

  it("keeps names and tags as plain work state", () => {
    const draft = setDraftTags(setDraftName(createEmptyDraft(), "  Tools  "), [
      "a",
      "b",
    ]);
    expect(toStudioDocument(draft).name).toBe("Tools");
    expect(toStudioDocument(draft).tags).toEqual(["a", "b"]);
  });

  it("canonicalizes through the shared Rust contract", async () => {
    const draft = setDraftName(createEmptyDraft(), "Tools");
    const receipt = await canonicalizeDraft(draft);
    expect(receipt.canonical_yaml).toBe("canonical");
    expect(compositionApi).toBeDefined();
  });

  it("provisions missing variable parameters without touching existing ones", () => {
    const existing = addParameter(createEmptyDraft(), {
      id: "month",
      type: "string",
      required: false,
    });
    if (!existing.ok) throw new Error("expected parameter");
    const provisioned = ensureParametersForVariables(existing.draft, {
      month: "date",
      region: "string",
    });
    // Existing parameters keep their declared shape; mismatches surface
    // as Rust-owned resolve diagnostics, never silent rebinds.
    expect(provisioned.parameters).toEqual([
      { id: "month", type: "string", required: false },
      { id: "region", type: "string", required: true },
    ]);
    expect(defaultParameterValues(provisioned)).toEqual({});
    const withDefault = upsertParameter(provisioned, {
      id: "region",
      type: "string",
      required: false,
      default: "eu",
    });
    if (!withDefault.ok) throw new Error("expected update");
    expect(defaultParameterValues(withDefault.draft)).toEqual({ region: "eu" });
  });

  it("restores a draft from a lint-normalized document", () => {
    const document: CompositionStudioDocument = {
      format: "ugoite.composition",
      format_version: 1,
      kind: "dashboard",
      name: "Monthly review",
      tags: ["finance"],
      spec: {
        parameters: [{
          id: "month",
          label: "Month",
          type: "date",
          required: true,
          default: "2026-01-01",
          format: "year-month",
        }],
        sources: [
          {
            kind: "saved_sql",
            id: "src-1",
            entry_id: "sql-1",
            revision_id: "sql-rev-1",
            expected_result: [{ name: "total", type: "float" }],
            variables: { month: { parameter: "month" } },
          },
          {
            kind: "entry_query",
            id: "src-2",
            form_id: "11111111-1111-4111-8111-111111111111",
            field_schema: [{ field_id: 1, field_type: "string" }],
            query: {
              filters: [],
              sort: [],
              projection: { kind: "fields", fields: [1] },
            },
          },
        ],
        components: [
          { kind: "table", id: "disp-1", label: "Totals", source: "src-1" },
          {
            kind: "metric",
            id: "disp-2",
            source: "src-2",
            value_field: { kind: "entry_field", field_id: 1 },
          },
        ],
        layout: {
          kind: "flow",
          rows: [{
            id: "main",
            items: [
              { kind: "component", component: "disp-1" },
              { kind: "component", component: "disp-2" },
            ],
          }],
        },
      },
    };
    const draft = draftFromDocument(document, {
      "src-1": "Monthly totals",
      "src-2": "Expenses",
    });

    expect(draft.name).toBe("Monthly review");
    expect(draft.sources.map((source) => source.name)).toEqual([
      "Monthly totals",
      "Expenses",
    ]);
    expect(draft.nextSourceSeq).toBe(3);
    expect(draft.nextDisplaySeq).toBe(3);
    // Human names never persist: the round trip keeps the normalized doc.
    expect(toStudioDocument(draft)).toEqual(document);
  });

  it("refuses unknown document kinds instead of approximating", () => {
    const document = {
      format: "ugoite.composition",
      format_version: 1,
      kind: "dashboard",
      name: "Tools",
      tags: [],
      spec: {
        parameters: [],
        sources: [{ kind: "future_source", id: "src-1" }],
        components: [],
        layout: { kind: "flow", rows: [{ id: "main", items: [] }] },
      },
    } as unknown as CompositionStudioDocument;
    expect(() => draftFromDocument(document, {})).toThrow();
  });

  it("ignores malformed studio seeds instead of approximating", () => {
    expect(studioSeedState(undefined)).toBeUndefined();
    expect(studioSeedState(null)).toBeUndefined();
    expect(studioSeedState({})).toBeUndefined();
    expect(studioSeedState({ seed: { kind: "chart" } })).toBeUndefined();
    expect(studioSeedState({ seed: { kind: "saved_sql" } })).toBeUndefined();
  });

  it("places text and parameter controls into new and existing rows", () => {
    let draft = createEmptyDraft();
    draft = addSavedSqlSource(draft, sqlSeed()).draft;
    const month = addParameter(draft, {
      id: "month",
      type: "date",
      required: true,
    });
    if (!month.ok) throw new Error("expected parameter");
    draft = month.draft;

    const heading = addTextDisplay(
      draft,
      { text: "Summary", style: "heading" },
      { rowId: null, rowIndex: 0, itemIndex: 0 },
    );
    expect(heading.draftId).toBe("disp-1");
    if (!heading.ok) throw new Error("expected text block");
    draft = heading.draft;

    const table = addTableDisplay(draft, "src-1", "Details", {
      rowId: null,
      rowIndex: 1,
      itemIndex: 0,
    });
    if (!table.ok) throw new Error("expected table block");
    draft = table.draft;

    const headingRow = draft.layoutRows.find((row) =>
      row.items.some((item) =>
        item.kind === "component" && item.draftId === "disp-1"
      )
    );
    if (!headingRow) throw new Error("expected heading row");
    const placed = placeParameterControl(draft, "month", {
      rowId: headingRow.id,
      rowIndex: 0,
      itemIndex: 0,
    });
    expect(placed.ok).toBe(true);
    if (!placed.ok) throw new Error("expected parameter placement");
    draft = placed.draft;

    const document = toStudioDocument(draft);
    expect(document.spec.components).toContainEqual({
      kind: "text",
      id: "disp-1",
      text: "Summary",
      style: "heading",
    });
    // Every component lands exactly once; the control references semantics.
    const items = document.spec.layout.rows.flatMap((row) => row.items);
    expect(items).toContainEqual({ kind: "component", component: "disp-1" });
    expect(items).toContainEqual({ kind: "component", component: "disp-2" });
    expect(items).toContainEqual({ kind: "parameter", parameter: "month" });
    expect(unplacedParameters(draft)).toHaveLength(0);

    // Placing twice or placing unknown parameters stays rejected.
    expect(placeParameterControl(draft, "month")).toEqual({
      ok: false,
      error: "parameter-already-placed",
    });
    expect(placeParameterControl(draft, "nope")).toEqual({
      ok: false,
      error: "unknown-parameter",
    });

    // Unplacing keeps the semantic parameter for the Data section.
    const unplaced = unplaceParameterControl(draft, "month");
    expect(unplaced.ok).toBe(true);
    if (!unplaced.ok) throw new Error("expected unplacement");
    expect(unplaced.draft.parameters.map((parameter) => parameter.id)).toEqual([
      "month",
    ]);
    expect(
      toStudioDocument(unplaced.draft).spec.layout.rows.flatMap((row) =>
        row.items
      ),
    ).not.toContainEqual({ kind: "parameter", parameter: "month" });
  });

  it("reorders rows and items within rows only", () => {
    let draft = createEmptyDraft();
    draft = addSavedSqlSource(draft, sqlSeed()).draft;
    const first = addTableDisplay(draft, "src-1", "One", {
      rowId: null,
      rowIndex: 0,
      itemIndex: 0,
    });
    if (!first.ok) throw new Error("expected block");
    const second = addTableDisplay(first.draft, "src-1", "Two", {
      rowId: null,
      rowIndex: 1,
      itemIndex: 0,
    });
    if (!second.ok) throw new Error("expected block");
    draft = second.draft;
    const rowIds = draft.layoutRows
      .filter((row) => row.items.length > 0)
      .map((row) => row.id);

    const movedRow = moveLayoutRow(draft, rowIds[1], "up");
    expect(movedRow.ok).toBe(true);
    if (!movedRow.ok) throw new Error("expected row move");
    expect(
      toStudioDocument(movedRow.draft).spec.layout.rows.map((row) => row.id),
    ).toEqual([rowIds[1], rowIds[0], "main"]);
    expect(moveLayoutRow(movedRow.draft, rowIds[1], "up")).toEqual({
      ok: false,
      error: "unknown-row",
    });

    // Within-row reorder swaps item order; row edges stay rejected and
    // cross-row moves go through insertion targets instead.
    const text = addTextDisplay(movedRow.draft, { text: "Note" }, {
      rowId: rowIds[1],
      rowIndex: 0,
      itemIndex: 1,
    });
    if (!text.ok) throw new Error("expected text block");
    const movedItem = moveLayoutItem(text.draft, rowIds[1], 1, "up");
    expect(movedItem.ok).toBe(true);
    if (!movedItem.ok) throw new Error("expected item move");
    const targetRow = toStudioDocument(movedItem.draft).spec.layout.rows.find((
      row,
    ) => row.id === rowIds[1]);
    expect(
      targetRow?.items.map((item) =>
        item.kind === "component" ? item.component : item.parameter
      ),
    ).toEqual(["disp-3", "disp-2"]);
    expect(moveLayoutItem(movedItem.draft, rowIds[1], 0, "up")).toEqual({
      ok: false,
      error: "unknown-block",
    });
    expect(moveLayoutItem(movedItem.draft, "nope", 0, "up")).toEqual({
      ok: false,
      error: "unknown-row",
    });
  });

  it("blocks parameter removal while a control is placed", () => {
    let draft = createEmptyDraft();
    const month = addParameter(draft, {
      id: "month",
      type: "date",
      required: true,
    });
    if (!month.ok) throw new Error("expected parameter");
    draft = month.draft;
    const placed = placeParameterControl(draft, "month");
    expect(placed.ok).toBe(true);
    if (!placed.ok) throw new Error("expected placement");
    expect(removeParameter(placed.draft, "month")).toEqual({
      ok: false,
      error: "parameter-referenced",
    });
    const unplaced = unplaceParameterControl(placed.draft, "month");
    if (!unplaced.ok) throw new Error("expected unplacement");
    expect(removeParameter(unplaced.draft, "month").ok).toBe(true);
  });

  it("keeps layout-only edits query-stable without refetching sources", () => {
    let draft = createEmptyDraft();
    draft = addSavedSqlSource(draft, sqlSeed()).draft;
    const table = addTableDisplay(draft, "src-1", "Details", {
      rowId: null,
      rowIndex: 0,
      itemIndex: 0,
    });
    if (!table.ok) throw new Error("expected block");
    draft = table.draft;
    const sources = JSON.stringify(toStudioDocument(draft).spec.sources);

    // Text insertion, row moves, and item moves never reshape sources.
    const text = addTextDisplay(draft, { text: "Note" }, {
      rowId: null,
      rowIndex: 0,
      itemIndex: 0,
    });
    if (!text.ok) throw new Error("expected text block");
    draft = text.draft;
    const rows = draft.layoutRows.filter((row) => row.items.length > 0);
    const movedRow = moveLayoutRow(draft, rows[0].id, "down");
    if (!movedRow.ok) throw new Error("expected row move");
    draft = movedRow.draft;
    const movedItem = moveLayoutItem(
      draft,
      draft.layoutRows.find((row) => row.items.length > 1)?.id ?? "main",
      0,
      "down",
    );
    const settled = movedItem.ok ? movedItem.draft : draft;
    expect(JSON.stringify(toStudioDocument(settled).spec.sources)).toBe(
      sources,
    );

    // Removing a text declaration leaves sources untouched as well.
    const removed = removeDisplay(settled, "disp-2");
    expect(removed.ok).toBe(true);
    if (!removed.ok) throw new Error("expected removal");
    expect(JSON.stringify(toStudioDocument(removed.draft).spec.sources)).toBe(
      sources,
    );
  });
});
