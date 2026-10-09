import { describe, expect, it, vi } from "vitest";
import {
  addEntryQuerySource,
  addMetricDisplay,
  addParameter,
  addSavedSqlSource,
  addTableDisplay,
  addTextDisplay,
  applyStudioSeed,
  canonicalizeDraft,
  type CompositionStudioDocument,
  createEmptyDraft,
  defaultParameterValues,
  displaysUsingSource,
  draftFromDocument,
  draftSaveReadiness,
  ensureParametersForVariables,
  moveDisplay,
  moveLayoutItem,
  moveLayoutRow,
  moveSource,
  MAX_ENTRY_PROJECTION_FIELDS,
  placeParameterControl,
  removeDisplay,
  removeParameter,
  removeSource,
  retargetParameterControl,
  setDraftName,
  setDraftTags,
  setEntryQueryFilters,
  setEntryQueryProjection,
  setEntryQuerySort,
  setMetricSource,
  setMetricValueField,
  setSavedSqlRevision,
  setTableSource,
  setTextContent,
  setTextStyle,
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

  it("drops empty layout rows when emitting the document", () => {
    // Palette insertions leave the initial empty row behind; empty rows
    // carry no placement meaning and the domain rejects them, so only
    // placed rows reach the document.
    const draft = createEmptyDraft("Untitled");
    const seeded = addTextDisplay(draft, { text: "Hello" });
    if (!seeded.ok || !seeded.draftId) throw new Error("insert failed");
    const document = toStudioDocument(seeded.draft);
    expect(document.spec.layout.rows).toEqual([{
      id: expect.any(String),
      items: [{ kind: "component", component: seeded.draftId }],
    }]);
    expect(
      document.spec.layout.rows.every((row) => row.items.length > 0),
    ).toBe(true);
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

  it("restores multi-row layout order with text and parameter placement", () => {
    // Labeled-fixture shape with semantic ids: an entry_query source with
    // parameter-bound filters, a saved_sql source with variable bindings, a
    // summary_title text block, and required parameter controls placed in
    // their own row ahead of two component rows.
    const document: CompositionStudioDocument = {
      format: "ugoite.composition",
      format_version: 1,
      kind: "dashboard",
      name: "Monthly expenses",
      tags: [],
      spec: {
        parameters: [
          {
            id: "month_start",
            label: "Start month",
            type: "date",
            required: true,
            format: "year-month",
          },
          {
            id: "month_end",
            label: "End month",
            type: "date",
            required: true,
            format: "year-month",
          },
        ],
        sources: [
          {
            kind: "entry_query",
            id: "expense_rows",
            form_id: "00000000-0000-7000-8000-000000000010",
            field_schema: [
              { field_id: 100, field_type: "date" },
              { field_id: 101, field_type: "string" },
              { field_id: 102, field_type: "double" },
            ],
            query: {
              filters: [
                {
                  field_id: 100,
                  operator: "gte",
                  value: { parameter: "month_start" },
                },
                {
                  field_id: 100,
                  operator: "lt",
                  value: { parameter: "month_end" },
                },
              ],
              sort: [{ field_id: 100, direction: "desc" }],
              projection: { kind: "fields", fields: [100, 101, 102] },
            },
          },
          {
            kind: "saved_sql",
            id: "month_total",
            entry_id: "00000000-0000-7000-8000-000000000020",
            revision_id: "00000000-0000-7000-8000-000000000021",
            expected_result: [{ name: "total", type: "float" }],
            variables: {
              month_start: { parameter: "month_start" },
              month_end: { parameter: "month_end" },
            },
          },
        ],
        components: [
          {
            kind: "text",
            id: "summary_title",
            text: "Monthly summary",
            style: "heading",
          },
          {
            kind: "metric",
            id: "total",
            label: "Monthly total",
            source: "month_total",
            value_field: { kind: "sql_column", name: "total" },
          },
          {
            kind: "table",
            id: "transactions",
            label: "Expense transactions",
            source: "expense_rows",
          },
        ],
        layout: {
          kind: "flow",
          rows: [
            {
              id: "controls",
              items: [
                { kind: "parameter", parameter: "month_start" },
                { kind: "parameter", parameter: "month_end" },
              ],
            },
            {
              id: "summary",
              items: [
                { kind: "component", component: "summary_title" },
                { kind: "component", component: "total" },
              ],
            },
            {
              id: "detail",
              items: [{ kind: "component", component: "transactions" }],
            },
          ],
        },
      },
    };
    const draft = draftFromDocument(document, {
      expense_rows: "Expenses",
      month_total: "Monthly totals",
    });

    // Rows keep document order and identity; nothing flattens to
    // declaration order. Semantic source ids remap to stable draft ids
    // while human names stay display-only.
    expect(draft.layoutRows.map((row) => row.id)).toEqual([
      "controls",
      "summary",
      "detail",
    ]);
    expect(draft.layoutRows[0].items).toEqual([
      { kind: "parameter", parameterId: "month_start" },
      { kind: "parameter", parameterId: "month_end" },
    ]);
    expect(draft.layoutRows[1].items).toEqual([
      { kind: "component", draftId: "disp-1" },
      { kind: "component", draftId: "disp-2" },
    ]);
    expect(draft.layoutRows[2].items).toEqual([
      { kind: "component", draftId: "disp-3" },
    ]);
    // Text content and style survive with no source binding.
    expect(draft.displays[0]).toEqual({
      kind: "text",
      draftId: "disp-1",
      text: "Monthly summary",
      style: "heading",
    });
    expect(draft.displays[1]).toMatchObject({
      kind: "metric",
      draftId: "disp-2",
      sourceDraftId: "src-2",
      valueField: { column: "total" },
    });
    expect(draft.displays[2]).toMatchObject({
      kind: "table",
      draftId: "disp-3",
      sourceDraftId: "src-1",
    });
    expect(draft.sources.map((source) => source.name)).toEqual([
      "Expenses",
      "Monthly totals",
    ]);
    // Parameter-bound entry_query filters round-trip verbatim.
    expect(toStudioDocument(draft).spec.sources[0]).toMatchObject({
      kind: "entry_query",
      query: {
        filters: [
          {
            field_id: 100,
            operator: "gte",
            value: { parameter: "month_start" },
          },
          { field_id: 100, operator: "lt", value: { parameter: "month_end" } },
        ],
      },
    });
    // Required parameters keep their placed controls: the restored draft
    // is save-ready for the update flow.
    expect(draftSaveReadiness(draft)).toEqual({ ready: true });

    // The re-emitted document keeps row order, per-row placement, and
    // component semantics with stable draft ids; every layout reference
    // resolves to a declaration.
    const emitted = toStudioDocument(draft);
    expect(emitted.spec.layout.rows.map((row) => row.id)).toEqual([
      "controls",
      "summary",
      "detail",
    ]);
    const emittedById = new Map(
      emitted.spec.components.map((component) => [component.id, component]),
    );
    for (const row of emitted.spec.layout.rows) {
      for (const item of row.items) {
        if (item.kind === "component") {
          expect(emittedById.has(item.component)).toBe(true);
        } else {
          expect(
            document.spec.parameters.some((parameter) =>
              parameter.id === item.parameter
            ),
          ).toBe(true);
        }
      }
    }
    expect(
      emitted.spec.layout.rows.flatMap((row) => row.items),
    ).toEqual([
      { kind: "parameter", parameter: "month_start" },
      { kind: "parameter", parameter: "month_end" },
      { kind: "component", component: "disp-1" },
      { kind: "component", component: "disp-2" },
      { kind: "component", component: "disp-3" },
    ]);
    expect(emitted.spec.components).toContainEqual({
      kind: "text",
      id: "disp-1",
      text: "Monthly summary",
      style: "heading",
    });
    expect(emitted.spec.components).toContainEqual({
      kind: "metric",
      id: "disp-2",
      label: "Monthly total",
      source: "src-2",
      value_field: { kind: "sql_column", name: "total" },
    });
    expect(emitted.spec.components).toContainEqual({
      kind: "table",
      id: "disp-3",
      label: "Expense transactions",
      source: "src-1",
    });
  });

  it("loads entry_query sources whose canonical query omits empty filters and sort", () => {
    // Canonical YAML drops empty collections, so saved documents arrive
    // without filters/sort keys; the loader defaults them like the domain.
    const document: CompositionStudioDocument = {
      format: "ugoite.composition",
      format_version: 1,
      kind: "dashboard",
      name: "Monthly expenses",
      tags: [],
      spec: {
        parameters: [],
        sources: [
          {
            kind: "entry_query",
            id: "rows",
            form_id: "form-1",
            field_schema: [{ field_id: 100, field_type: "double" }],
            query: {
              projection: { kind: "preview" },
            },
          },
        ],
        components: [{ kind: "table", id: "rows", source: "rows" }],
        layout: {
          kind: "flow",
          rows: [
            {
              id: "main",
              items: [{ kind: "component", component: "rows" }],
            },
          ],
        },
      },
    };
    const draft = draftFromDocument(document, {});
    expect(draft.sources[0]).toMatchObject({
      kind: "entry_query",
      query: { filters: [], sort: [] },
    });
    expect(draftSaveReadiness(draft)).toEqual({ ready: true });
  });

  it("refuses unknown future component, layout, and value-field kinds instead of approximating", () => {
    const base = (): CompositionStudioDocument => ({
      format: "ugoite.composition",
      format_version: 1,
      kind: "dashboard",
      name: "Monthly expenses",
      tags: [],
      spec: {
        parameters: [{ id: "month", type: "date", required: false }],
        sources: [
          {
            kind: "saved_sql",
            id: "month_total",
            entry_id: "sql-1",
            revision_id: "sql-rev-1",
            expected_result: [{ name: "total", type: "float" }],
            variables: {},
          },
        ],
        components: [
          {
            kind: "metric",
            id: "total",
            source: "month_total",
            value_field: { kind: "sql_column", name: "total" },
          },
        ],
        layout: {
          kind: "flow",
          rows: [{
            id: "main",
            items: [{ kind: "component", component: "total" }],
          }],
        },
      },
    });
    const withComponents = (
      components: unknown,
      rows?: unknown,
    ): CompositionStudioDocument => {
      const document = base();
      (document.spec as { components: unknown }).components = components;
      if (rows !== undefined) {
        (document.spec as { layout: { rows: unknown } }).layout = {
          kind: "flow",
          rows,
        } as never;
      }
      return document;
    };
    const withRows = (rows: unknown): CompositionStudioDocument => {
      const document = base();
      (document.spec as { layout: { rows: unknown } }).layout = {
        kind: "flow",
        rows,
      } as never;
      return document;
    };

    // Unknown future component kinds fail closed even when the layout
    // references them; text itself is a known kind and loads (covered by
    // the multi-row round trip above).
    expect(() =>
      draftFromDocument(
        withComponents(
          [{ kind: "chart", id: "future", source: "month_total" }],
          [{
            id: "main",
            items: [{ kind: "component", component: "future" }],
          }],
        ),
        {},
      )
    ).toThrow();
    // Unknown future layout item kinds fail closed.
    expect(() =>
      draftFromDocument(
        withRows([{
          id: "main",
          items: [{ kind: "widget", widget: "future" }],
        }]),
        {},
      )
    ).toThrow();
    // Unknown future metric value-field kinds fail closed.
    expect(() =>
      draftFromDocument(
        withComponents([{
          kind: "metric",
          id: "total",
          source: "month_total",
          value_field: { kind: "future_field" },
        }]),
        {},
      )
    ).toThrow();
    // Genuinely invalid text shapes fail closed: unknown styles and
    // non-string content are never approximated.
    const textRow = [{
      id: "main",
      items: [{ kind: "component", component: "summary_title" }],
    }];
    expect(() =>
      draftFromDocument(
        withComponents([{
          kind: "text",
          id: "summary_title",
          text: "Monthly summary",
          style: "banner",
        }], textRow),
        {},
      )
    ).toThrow();
    expect(() =>
      draftFromDocument(
        withComponents([{
          kind: "text",
          id: "summary_title",
          text: 42,
          style: "heading",
        }], textRow),
        {},
      )
    ).toThrow();
    // Dangling layout references fail closed on both sides.
    expect(() =>
      draftFromDocument(
        withRows([{
          id: "main",
          items: [{ kind: "component", component: "missing" }],
        }]),
        {},
      )
    ).toThrow();
    expect(() =>
      draftFromDocument(
        withRows([{
          id: "main",
          items: [{ kind: "parameter", parameter: "missing" }],
        }]),
        {},
      )
    ).toThrow();
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
    // Empty rows never reach the document; only placed rows are emitted.
    expect(
      toStudioDocument(movedRow.draft).spec.layout.rows.map((row) => row.id),
    ).toEqual([rowIds[1], rowIds[0]]);
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

  it("retargets metric source and value field together", () => {
    let draft = createEmptyDraft();
    draft = addSavedSqlSource(draft, sqlSeed()).draft;
    draft = addEntryQuerySource(draft, entrySeed()).draft;
    const metric = addMetricDisplay(
      draft,
      "src-1",
      { column: "total" },
      "Total",
    );
    if (!metric.ok) throw new Error("expected metric");
    draft = metric.draft;

    const moved = setMetricSource(draft, "disp-1", "src-2", { fieldId: 1 });
    expect(moved.ok).toBe(true);
    if (!moved.ok) throw new Error("expected source change");
    expect(
      toStudioDocument(moved.draft).spec.components[0],
    ).toMatchObject({
      kind: "metric",
      source: "src-2",
      value_field: { kind: "entry_field", field_id: 1 },
    });

    const rebound = setMetricValueField(moved.draft, "disp-1", {
      column: "total",
    });
    expect(rebound.ok).toBe(true);
    if (!rebound.ok) throw new Error("expected value change");
    expect(
      toStudioDocument(rebound.draft).spec.components[0],
    ).toMatchObject({
      value_field: { kind: "sql_column", name: "total" },
    });

    // Existing guards stay: wrong kinds and unknown identities fail closed.
    expect(setMetricSource(draft, "disp-1", "src-9", { column: "total" }))
      .toEqual({ ok: false, error: "unknown-source" });
    expect(setMetricSource(draft, "nope", "src-1", { column: "total" }))
      .toEqual({ ok: false, error: "unknown-display" });
    expect(setMetricValueField(draft, "nope", { column: "total" })).toEqual({
      ok: false,
      error: "unknown-display",
    });
  });

  it("retargets table source with existing guards", () => {
    let draft = createEmptyDraft();
    draft = addSavedSqlSource(draft, sqlSeed()).draft;
    draft = addEntryQuerySource(draft, entrySeed()).draft;
    const table = addTableDisplay(draft, "src-2", "Details");
    if (!table.ok) throw new Error("expected table");
    draft = table.draft;

    const moved = setTableSource(draft, "disp-1", "src-1");
    expect(moved.ok).toBe(true);
    if (!moved.ok) throw new Error("expected source change");
    expect(toStudioDocument(moved.draft).spec.components[0]).toMatchObject({
      kind: "table",
      source: "src-1",
      label: "Details",
    });

    expect(setTableSource(draft, "disp-1", "src-9")).toEqual({
      ok: false,
      error: "unknown-source",
    });
    expect(setTableSource(draft, "nope", "src-1")).toEqual({
      ok: false,
      error: "unknown-display",
    });
  });

  it("edits text content and style with a fixed enum", () => {
    let draft = createEmptyDraft();
    draft = addSavedSqlSource(draft, sqlSeed()).draft;
    const text = addTextDisplay(draft, { text: "Summary", style: "heading" });
    if (!text.ok) throw new Error("expected text");
    draft = text.draft;
    const sources = JSON.stringify(toStudioDocument(draft).spec.sources);

    const edited = setTextContent(draft, "disp-1", "New heading");
    expect(edited.ok).toBe(true);
    if (!edited.ok) throw new Error("expected content change");
    const styled = setTextStyle(edited.draft, "disp-1", "title");
    expect(styled.ok).toBe(true);
    if (!styled.ok) throw new Error("expected style change");
    expect(toStudioDocument(styled.draft).spec.components[0]).toMatchObject({
      kind: "text",
      text: "New heading",
      style: "title",
    });
    // Text edits never reshape sources, so they never refetch.
    expect(JSON.stringify(toStudioDocument(styled.draft).spec.sources)).toBe(
      sources,
    );

    expect(setTextStyle(styled.draft, "disp-1", "banner" as never)).toEqual({
      ok: false,
      error: "invalid-style",
    });
    expect(setTextContent(draft, "nope", "x")).toEqual({
      ok: false,
      error: "unknown-display",
    });
  });

  it("retargets parameter controls in place without touching declarations", () => {
    let draft = createEmptyDraft();
    const month = addParameter(draft, {
      id: "month",
      label: "Month",
      type: "date",
      required: true,
    });
    if (!month.ok) throw new Error("expected month");
    draft = month.draft;
    const region = addParameter(draft, {
      id: "region",
      label: "Region",
      type: "string",
      required: false,
    });
    if (!region.ok) throw new Error("expected region");
    draft = region.draft;
    const quarter = addParameter(draft, {
      id: "quarter",
      label: "Quarter",
      type: "string",
      required: false,
    });
    if (!quarter.ok) throw new Error("expected quarter");
    draft = quarter.draft;
    const heading = addTextDisplay(draft, { text: "Summary" });
    if (!heading.ok) throw new Error("expected text");
    draft = heading.draft;
    const placed = placeParameterControl(draft, "month");
    if (!placed.ok) throw new Error("expected placement");
    draft = placed.draft;
    const before = draft.layoutRows.map((row) => ({
      ...row,
      items: [...row.items],
    }));

    const retargeted = retargetParameterControl(draft, "month", "region");
    expect(retargeted.ok).toBe(true);
    if (!retargeted.ok) throw new Error("expected retarget");
    const items = toStudioDocument(retargeted.draft).spec.layout.rows.flatMap(
      (row) => row.items,
    );
    expect(items).toContainEqual({ kind: "parameter", parameter: "region" });
    expect(items).not.toContainEqual({
      kind: "parameter",
      parameter: "month",
    });
    // Same row, same position; declarations untouched with no label override.
    expect(retargeted.draft.layoutRows.map((row) => row.id)).toEqual(
      before.map((row) => row.id),
    );
    expect(
      retargeted.draft.parameters.map((parameter) => parameter.id),
    ).toEqual(["month", "region", "quarter"]);

    expect(retargetParameterControl(draft, "month", "month")).toEqual({
      ok: true,
      draft,
    });
    expect(retargetParameterControl(draft, "month", "nope")).toEqual({
      ok: false,
      error: "unknown-parameter",
    });
    expect(retargetParameterControl(draft, "nope", "region")).toEqual({
      ok: false,
      error: "unknown-parameter",
    });
    expect(retargetParameterControl(draft, "region", "month")).toEqual({
      ok: false,
      error: "parameter-already-placed",
    });
    expect(retargetParameterControl(draft, "region", "quarter")).toEqual({
      ok: false,
      error: "unknown-block",
    });
  });

  it("maps entry-query filter edits onto the draft with existing operators", () => {
    let draft = createEmptyDraft();
    draft = addEntryQuerySource(draft, entrySeed()).draft;
    const applied = setEntryQueryFilters(draft, "src-1", [
      { field_id: 1, operator: "gte", value: "2026-10-01" },
    ]);
    expect(applied.ok).toBe(true);
    if (!applied.ok) throw new Error("expected filter update");
    expect(toStudioDocument(applied.draft).spec.sources[0]).toMatchObject({
      kind: "entry_query",
      query: {
        filters: [{ field_id: 1, operator: "gte", value: "2026-10-01" }],
      },
    });
  });

  it("rejects entry-query filter edits with unknown operators", () => {
    let draft = createEmptyDraft();
    draft = addEntryQuerySource(draft, entrySeed()).draft;
    expect(
      setEntryQueryFilters(draft, "src-1", [
        {
          field_id: 1,
          operator: "starts_with" as "equals",
          value: "october",
        },
      ]),
    ).toEqual({ ok: false, error: "invalid-query" });
    expect(
      setEntryQueryFilters(draft, "src-1", [
        { field_id: 1.5, operator: "equals", value: "october" },
      ]),
    ).toEqual({ ok: false, error: "invalid-query" });
    expect(
      setEntryQueryFilters(draft, "src-1", [
        { field_id: 1, operator: "equals", value: { nested: true } },
      ]),
    ).toEqual({ ok: false, error: "invalid-query" });
    expect(setEntryQueryFilters(draft, "nope", [])).toEqual({
      ok: false,
      error: "unknown-source",
    });
    // Parameter-bound values keep their shape through the same channel.
    const bound = setEntryQueryFilters(draft, "src-1", [
      { field_id: 1, operator: "gte", value: { parameter: "month" } },
    ]);
    expect(bound.ok).toBe(true);
  });

  it("rejects entry-query sort edits with unknown directions", () => {
    let draft = createEmptyDraft();
    draft = addEntryQuerySource(draft, entrySeed()).draft;
    const applied = setEntryQuerySort(draft, "src-1", [
      { field_id: 1, direction: "desc" },
    ]);
    expect(applied.ok).toBe(true);
    expect(
      setEntryQuerySort(draft, "src-1", [
        { field_id: 1, direction: "newest" as "asc" },
      ]),
    ).toEqual({ ok: false, error: "invalid-query" });
    expect(setEntryQuerySort(draft, "nope", [])).toEqual({
      ok: false,
      error: "unknown-source",
    });
  });

  it("replaces entry-query projections between preview and fields", () => {
    let draft = createEmptyDraft();
    draft = addEntryQuerySource(draft, entrySeed()).draft;
    const fields = setEntryQueryProjection(draft, "src-1", {
      kind: "fields",
      fields: [1],
    });
    expect(fields.ok).toBe(true);
    if (!fields.ok) throw new Error("expected projection update");
    expect(toStudioDocument(fields.draft).spec.sources[0]).toMatchObject({
      kind: "entry_query",
      query: { projection: { kind: "fields", fields: [1] } },
    });
    expect(
      setEntryQueryProjection(draft, "src-1", {
        kind: "fields",
        fields: [1.5],
      }),
    ).toEqual({ ok: false, error: "invalid-query" });
    expect(
      setEntryQueryProjection(draft, "src-1", {
        kind: "everything" as "preview",
      }),
    ).toEqual({ ok: false, error: "invalid-query" });
  });

  it("keeps EntryQuery metric fields in the source projection", () => {
    let draft = createEmptyDraft();
    draft = addEntryQuerySource(draft, entrySeed()).draft;
    const metric = addMetricDisplay(draft, "src-1", { fieldId: 1 });
    expect(metric.ok).toBe(true);
    if (!metric.ok) throw new Error("expected metric");
    draft = metric.draft;
    expect(toStudioDocument(draft).spec.sources[0]).toMatchObject({
      kind: "entry_query",
      query: { projection: { kind: "fields", fields: [1] } },
    });

    const rebound = setMetricValueField(draft, metric.draftId!, { fieldId: 2 });
    expect(rebound.ok).toBe(true);
    if (!rebound.ok) throw new Error("expected value field change");
    draft = rebound.draft;
    expect(toStudioDocument(draft).spec.sources[0]).toMatchObject({
      query: { projection: { kind: "fields", fields: [1, 2] } },
    });

    const preview = setEntryQueryProjection(draft, "src-1", {
      kind: "preview",
    });
    expect(preview.ok).toBe(true);
    if (!preview.ok) throw new Error("expected projection change");
    expect(toStudioDocument(preview.draft).spec.sources[0]).toMatchObject({
      query: { projection: { kind: "fields", fields: [2] } },
    });
  });

  it("rejects a metric field that would exceed the EntryQuery projection limit", () => {
    const fields = Array.from({ length: MAX_ENTRY_PROJECTION_FIELDS + 1 }, (_, index) => ({
      field_id: index + 1,
      field_type: "integer",
    }));
    let draft = createEmptyDraft();
    draft = addEntryQuerySource(draft, {
      ...entrySeed(),
      fieldSchema: fields,
      query: {
        ...entrySeed().query,
        projection: {
          kind: "fields",
          fields: fields.slice(0, MAX_ENTRY_PROJECTION_FIELDS).map((entry) =>
            entry.field_id
          ),
        },
      },
    }).draft;

    expect(
      addMetricDisplay(draft, "src-1", {
        fieldId: MAX_ENTRY_PROJECTION_FIELDS + 1,
      }),
    ).toEqual({ ok: false, error: "invalid-query" });
  });

  it("adds a metric field when retargeting to an EntryQuery source", () => {
    let draft = createEmptyDraft();
    draft = addSavedSqlSource(draft, sqlSeed()).draft;
    draft = addEntryQuerySource(draft, entrySeed()).draft;
    const metric = addMetricDisplay(draft, "src-1", { column: "total" });
    if (!metric.ok || !metric.draftId) throw new Error("expected metric");
    const moved = setMetricSource(
      metric.draft,
      metric.draftId,
      "src-2",
      { fieldId: 2 },
    );
    expect(moved.ok).toBe(true);
    if (!moved.ok) throw new Error("expected source change");
    expect(toStudioDocument(moved.draft).spec.sources[1]).toMatchObject({
      kind: "entry_query",
      query: { projection: { kind: "fields", fields: [2] } },
    });
  });

  it("points saved sql sources at an exact revision preserving bindings", () => {
    let draft = createEmptyDraft();
    draft = addSavedSqlSource(draft, {
      ...sqlSeed(),
      variables: {
        month: { parameter: "period" },
        gone: { parameter: "gone" },
      },
    }).draft;
    const updated = setSavedSqlRevision(draft, "src-1", {
      revisionId: "rev-2",
      expectedResult: [{ name: "total", type: "float" }],
      variableNames: ["month", "region"],
    });
    expect(updated.ok).toBe(true);
    if (!updated.ok) throw new Error("expected revision update");
    const source = updated.draft.sources[0];
    if (source.kind !== "saved_sql") throw new Error("expected saved sql");
    expect(source.revisionId).toBe("rev-2");
    // Existing bindings keep their parameter; new variables bind
    // same-named parameters; removed variables drop their bindings.
    expect(source.variables).toEqual({
      month: { parameter: "period" },
      region: { parameter: "region" },
    });
    expect(updated.draft.sources[0]).toMatchObject({
      kind: "saved_sql",
      revisionId: "rev-2",
    });
    expect(
      setSavedSqlRevision(draft, "src-1", {
        revisionId: "  ",
        expectedResult: [],
        variableNames: [],
      }),
    ).toEqual({ ok: false, error: "invalid-query" });
    expect(
      setSavedSqlRevision(draft, "nope", {
        revisionId: "rev-2",
        expectedResult: [],
        variableNames: [],
      }),
    ).toEqual({ ok: false, error: "unknown-source" });
  });

  it("seeds a form entry point with an entry-query source and a default table", () => {
    const { draft, draftId } = applyStudioSeed(createEmptyDraft(), {
      kind: "entry_query",
      seed: entrySeed(),
    });
    expect(draftId).toBe("src-1");
    expect(draft.name).toBe("Expenses");
    expect(draft.sources).toHaveLength(1);
    // One default Table on the new source, placed in a single row: the
    // Studio opens with a visible block, never zero-display.
    expect(draft.displays).toHaveLength(1);
    expect(draft.displays[0]).toMatchObject({
      kind: "table",
      draftId: "disp-1",
      sourceDraftId: "src-1",
    });
    expect(draft.layoutRows).toHaveLength(1);
    expect(draft.layoutRows[0].items).toEqual([
      { kind: "component", draftId: "disp-1" },
    ]);
    expect(draftSaveReadiness(draft)).toEqual({ ready: true });
  });

  it("seeds a saved sql entry point with mapped variables and placed controls", () => {
    const { draft, draftId } = applyStudioSeed(createEmptyDraft(), {
      kind: "saved_sql",
      seed: {
        entryId: "sql-1",
        revisionId: "rev-1",
        name: "Monthly",
        expectedResult: [{ name: "total", type: "float" as const }],
        variables: {
          month_start: { parameter: "month_start" },
          month_end: { parameter: "month_end" },
        },
        variableTypes: { month_start: "date", month_end: "date" },
        variableDefaults: { month_start: "2026-01-01" },
      },
    });
    expect(draftId).toBe("src-1");
    expect(draft.name).toBe("Monthly");
    const source = draft.sources[0];
    if (source.kind !== "saved_sql") throw new Error("expected saved sql");
    expect(source.revisionId).toBe("rev-1");
    // Parameters map from the exact-revision variables with server types.
    expect(draft.parameters.map((parameter) => parameter.id)).toEqual([
      "month_start",
      "month_end",
    ]);
    // Run-time values ride along as defaults so the seeded Studio opens
    // showing the same result; variables without a run value stay defaultless.
    expect(
      draft.parameters.map((parameter) => parameter.default ?? null),
    ).toEqual(["2026-01-01", null]);
    // Controls land only because variables exist, ahead of the table row
    // item; the table keeps the draft save-ready and never zero-display.
    const items = draft.layoutRows.flatMap((row) => row.items);
    expect(items).toEqual([
      { kind: "parameter", parameterId: "month_start" },
      { kind: "parameter", parameterId: "month_end" },
      { kind: "component", draftId: "disp-1" },
    ]);
    expect(draft.displays[0]).toMatchObject({
      kind: "table",
      sourceDraftId: "src-1",
      label: "Monthly",
    });
    expect(unplacedParameters(draft)).toHaveLength(0);
    expect(draftSaveReadiness(draft)).toEqual({ ready: true });
  });

  it("seeds saved sql without variables with a table and no parameter controls", () => {
    const { draft } = applyStudioSeed(createEmptyDraft(), {
      kind: "saved_sql",
      seed: sqlSeed(),
    });
    expect(draft.parameters).toHaveLength(0);
    const items = draft.layoutRows.flatMap((row) => row.items);
    expect(items).toEqual([{ kind: "component", draftId: "disp-1" }]);
    expect(draftSaveReadiness(draft)).toEqual({ ready: true });
  });

  it("blocks saves until the name, refs, and layout are all ready", () => {
    expect(draftSaveReadiness(createEmptyDraft())).toEqual({
      ready: false,
      reason: "name",
    });
    expect(draftSaveReadiness(createEmptyDraft("  "))).toEqual({
      ready: false,
      reason: "name",
    });
    let draft = createEmptyDraft("Monthly");
    expect(draftSaveReadiness(draft)).toEqual({
      ready: false,
      reason: "sources",
    });

    // A source-only draft cannot save: no layout item yet.
    draft = addSavedSqlSource(draft, sqlSeed()).draft;
    expect(draftSaveReadiness(draft)).toEqual({
      ready: false,
      reason: "layout",
    });

    // A declared but unplaced component dangles instead.
    const tabled = addTableDisplay(draft, "src-1");
    if (!tabled.ok) throw new Error("expected table block");
    draft = { ...tabled.draft, layoutRows: [] };
    expect(draftSaveReadiness(draft)).toEqual({
      ready: false,
      reason: "refs",
    });

    // Unknown display sources, dangling items, and duplicate placements
    // all block with the refs reason.
    draft = {
      ...tabled.draft,
      displays: [
        {
          kind: "table",
          draftId: "disp-1",
          sourceDraftId: "src-9",
        },
      ],
    };
    expect(draftSaveReadiness(draft)).toEqual({
      ready: false,
      reason: "refs",
    });
    draft = {
      ...tabled.draft,
      layoutRows: [{
        id: "main",
        items: [{ kind: "component", draftId: "disp-9" }],
      }],
    };
    expect(draftSaveReadiness(draft)).toEqual({
      ready: false,
      reason: "refs",
    });
    draft = {
      ...tabled.draft,
      layoutRows: [{
        id: "main",
        items: [
          { kind: "component", draftId: "disp-1" },
          { kind: "component", draftId: "disp-1" },
        ],
      }],
    };
    expect(draftSaveReadiness(draft)).toEqual({
      ready: false,
      reason: "refs",
    });

    // The placed table is save-ready again.
    draft = tabled.draft;
    expect(draftSaveReadiness(draft)).toEqual({ ready: true });

    // Required parameters without a default need a placed control.
    const month = addParameter(draft, {
      id: "month",
      type: "date",
      required: true,
    });
    if (!month.ok) throw new Error("expected parameter");
    expect(draftSaveReadiness(month.draft)).toEqual({
      ready: false,
      reason: "refs",
    });
    const placed = placeParameterControl(month.draft, "month");
    if (!placed.ok) throw new Error("expected parameter placement");
    expect(draftSaveReadiness(placed.draft)).toEqual({ ready: true });
  });
});
