import { describe, expect, it, vi } from "vitest";
import {
  addEntryQuerySource,
  addInputComponent,
  addMetricDisplay,
  addSavedSqlSource,
  addTableDisplay,
  addTextDisplay,
  addVariable,
  applyStudioSeed,
  canonicalizeDraft,
  type CompositionStudioDocument,
  createEmptyDraft,
  defaultVariableValues,
  displaysUsingSource,
  draftFromDocument,
  draftSaveReadiness,
  MAX_ENTRY_PROJECTION_FIELDS,
  moveDisplay,
  moveLayoutItem,
  moveLayoutRow,
  moveSource,
  removeDisplay,
  removeInputComponent,
  removeSource,
  removeVariable,
  savedSqlVariableTypesFromFields,
  setDisplayLabel,
  setDraftName,
  setDraftTags,
  setEntryQueryDisplaySystemFields,
  setEntryQueryFilters,
  setEntryQueryProjection,
  setEntryQuerySort,
  setInputLabel,
  setInputVariable,
  setMetricColumn,
  setMetricPresentation,
  setMetricSource,
  setSavedSqlRevision,
  setSavedSqlVariableBinding,
  setSavedSqlVariableTypes,
  setTableSource,
  setTextContent,
  setTextStyle,
  studioSeedState,
  toStudioDocument,
  upsertVariable,
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
}));

const sqlSeed = () => ({
  entryId: "sql-1",
  revisionId: "rev-1",
  name: "Monthly totals",
  expectedResult: [{ name: "total", type: "float" as const }],
  variables: {},
  variableTypes: {},
});

const entrySeed = () => ({
  formId: "form-1",
  name: "Expenses",
  fieldSchema: [{ field_id: 1, field_type: "string" }],
  query: { filters: [], sort: [], projection: { kind: "preview" as const } },
});

describe("composition draft model", () => {
  it("accepts Saved SQL variables with prototype property names safely", () => {
    const variableTypes = savedSqlVariableTypesFromFields({
      variables: [
        { name: "__proto__", type: "date" },
        { name: "constructor", type: "integer" },
        { name: "toString", type: "string" },
      ],
    });

    expect(Object.getPrototypeOf(variableTypes)).toBe(Object.prototype);
    expect(Object.keys(variableTypes)).toEqual([
      "__proto__",
      "constructor",
      "toString",
    ]);
    expect(Object.hasOwn(variableTypes, "__proto__")).toBe(true);
    expect(variableTypes["__proto__"]).toBe("date");
    expect(variableTypes.constructor).toBe("integer");
    expect(variableTypes.toString).toBe("string");
    expect(() =>
      savedSqlVariableTypesFromFields({
        variables: [
          { name: "__proto__", type: "date" },
          { name: "__proto__", type: "string" },
        ],
      })
    ).toThrow("Invalid Saved SQL variable");
  });

  it("binds prototype-named Saved SQL variables as ordinary own properties", () => {
    const variableTypes = savedSqlVariableTypesFromFields({
      variables: [{ name: "__proto__", type: "date" }],
    });
    const declared = addVariable(createEmptyDraft(), {
      id: "period",
      type: "date",
      required: true,
    });
    if (!declared.ok) throw new Error("expected declared variable");
    const draft = addSavedSqlSource(declared.draft, {
      ...sqlSeed(),
      variableTypes,
    }).draft;

    const bound = setSavedSqlVariableBinding(
      draft,
      "src-1",
      "__proto__",
      "period",
    );
    expect(bound.ok).toBe(true);
    if (!bound.ok) throw new Error("expected explicit SQL variable binding");
    const source = bound.draft.sources[0];
    if (source.kind !== "saved_sql") throw new Error("expected Saved SQL");
    expect(Object.getPrototypeOf(source.variables)).toBe(Object.prototype);
    expect(Object.hasOwn(source.variables, "__proto__")).toBe(true);
    expect(source.variables["__proto__"]).toEqual({ variable: "period" });

    const updated = setSavedSqlRevision(bound.draft, "src-1", {
      revisionId: "rev-2",
      expectedResult: [{ name: "total", type: "float" }],
      variableTypes: savedSqlVariableTypesFromFields({
        variables: [{ name: "__proto__", type: "date" }],
      }),
    });
    expect(updated.ok).toBe(true);
    if (!updated.ok) throw new Error("expected exact revision update");
    const updatedSource = updated.draft.sources[0];
    if (updatedSource.kind !== "saved_sql") {
      throw new Error("expected Saved SQL");
    }
    expect(Object.hasOwn(updatedSource.variables, "__proto__")).toBe(true);
    expect(updatedSource.variables["__proto__"]).toEqual({
      variable: "period",
    });
  });

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
      "total",
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
      source: "src-1",
      column: "total",
    });
  });

  it("clears numeric presentation when a Metric changes to a nonnumeric result", () => {
    const source = addSavedSqlSource(createEmptyDraft(), {
      ...sqlSeed(),
      expectedResult: [
        { name: "total", type: "float" },
        { name: "day", type: "date" },
      ],
    });
    const metric = addMetricDisplay(source.draft, source.draftId, "total");
    if (!metric.ok || !metric.draftId) throw new Error("expected Metric");
    const formatted = setMetricPresentation(metric.draft, metric.draftId, {
      kind: "number",
      unit: "USD",
      fractionDigits: 2,
    });
    if (!formatted.ok) throw new Error("expected numeric presentation");

    const changed = setMetricColumn(formatted.draft, metric.draftId, "day");
    if (!changed.ok) throw new Error("expected date column");
    expect(changed.draft.displays[0]).not.toHaveProperty("presentation");

    expect(setMetricPresentation(changed.draft, metric.draftId, {
      kind: "number",
      unit: "USD",
    })).toMatchObject({ ok: false, error: "invalid-query" });
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

  it("guards variables against duplicates and silent unbinding", () => {
    let draft = createEmptyDraft();
    const variable = { id: "month", type: "date" as const, required: true };
    const added = addVariable(draft, variable);
    expect(added.ok).toBe(true);
    if (!added.ok) throw new Error("expected variable");
    draft = added.draft;
    expect(addVariable(draft, variable)).toEqual({
      ok: false,
      error: "duplicate-variable",
    });
    expect(removeVariable(draft, "nope")).toEqual({
      ok: false,
      error: "unknown-variable",
    });

    const updated = upsertVariable(draft, { ...variable, required: false });
    expect(updated.ok).toBe(true);

    // A variable bound by a source variable cannot be removed silently.
    const seeded = addSavedSqlSource(createEmptyDraft(), {
      ...sqlSeed(),
      variables: { month: { variable: "month" } },
    }).draft;
    const withParam = addVariable(seeded, variable);
    if (!withParam.ok) throw new Error("expected variable");
    expect(removeVariable(withParam.draft, "month")).toEqual({
      ok: false,
      error: "variable-referenced",
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

  it("keeps variable declarations explicit when adding Saved SQL", () => {
    const existing = addVariable(createEmptyDraft(), {
      id: "month",
      type: "string",
      required: false,
    });
    if (!existing.ok) throw new Error("expected variable");
    const withSource = addSavedSqlSource(existing.draft, {
      ...sqlSeed(),
      variables: {},
      variableTypes: { month: "date", region: "string" },
    }).draft;
    expect(withSource.variables).toEqual([
      { id: "month", type: "string", required: false },
    ]);
    expect(defaultVariableValues(withSource)).toEqual({});
    const withDefault = upsertVariable(withSource, {
      id: "region",
      type: "string",
      required: false,
      default: "eu",
    });
    if (!withDefault.ok) throw new Error("expected update");
    expect(defaultVariableValues(withDefault.draft)).toEqual({ region: "eu" });
  });

  it("restores a draft from a lint-normalized document", () => {
    const document: CompositionStudioDocument = {
      format: "ugoite.composition",
      format_version: 1,
      kind: "dashboard",
      name: "Monthly review",
      tags: ["finance"],
      spec: {
        variables: [{
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
            variables: { month: { variable: "month" } },
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
            source: "src-1",
            column: "total",
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
    let draft = draftFromDocument(document, {
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
        variables: [],
        sources: [{ kind: "future_source", id: "src-1" }],
        components: [],
        layout: { kind: "flow", rows: [{ id: "main", items: [] }] },
      },
    } as unknown as CompositionStudioDocument;
    expect(() => draftFromDocument(document, {})).toThrow();
  });

  it("never falls back to persisted source identities as visible names", () => {
    const sourceId = "internal-source-identity";
    const document: CompositionStudioDocument = {
      format: "ugoite.composition",
      format_version: 1,
      kind: "dashboard",
      name: "Tools",
      tags: [],
      spec: {
        variables: [],
        sources: [{
          kind: "saved_sql",
          id: sourceId,
          entry_id: "internal-entry-identity",
          revision_id: "internal-revision-identity",
          expected_result: [{ name: "total", type: "float" }],
          variables: {},
        }],
        components: [{ kind: "table", id: "rows", source: sourceId }],
        layout: {
          kind: "flow",
          rows: [{
            id: "main",
            items: [{ kind: "component", component: "rows" }],
          }],
        },
      },
    };
    const draft = draftFromDocument(document, {});
    expect(draft.sources[0].name).toBeTruthy();
    expect(draft.sources[0].name).not.toContain(sourceId);
    expect(draft.sources[0].name).not.toContain("internal-entry-identity");
    expect(draft.sources[0].name).not.toContain("internal-revision-identity");
  });

  it("keeps generated display ids clear of preserved Input ids", () => {
    const document: CompositionStudioDocument = {
      format: "ugoite.composition",
      format_version: 1,
      kind: "dashboard",
      name: "Tools",
      tags: [],
      spec: {
        variables: [{
          id: "title",
          type: "string",
          required: false,
        }],
        sources: [],
        components: [
          {
            kind: "text",
            id: "heading",
            content: { kind: "literal", text: "Heading" },
            style: "heading",
          },
          {
            kind: "input",
            id: "disp-1",
            variable: "title",
          },
        ],
        layout: {
          kind: "flow",
          rows: [{
            id: "main",
            items: [
              { kind: "component", component: "disp-1" },
              { kind: "component", component: "heading" },
            ],
          }],
        },
      },
    };

    const draft = draftFromDocument(document, {});
    const restored = toStudioDocument(draft);
    expect(draft.displays[0].draftId).toBe("disp-2");
    expect(restored.spec.components.map((component) => component.id)).toEqual([
      "disp-2",
      "disp-1",
    ]);
    expect(restored.spec.layout.rows[0].items).toEqual([
      { kind: "component", component: "disp-1" },
      { kind: "component", component: "disp-2" },
    ]);

    const added = addTextDisplay(draft, { text: "Note" });
    if (!added.ok || !added.draftId) throw new Error("expected text block");
    const afterInsert = toStudioDocument(added.draft);
    expect(added.draftId).toBe("disp-3");
    expect(
      new Set(afterInsert.spec.components.map((component) => component.id)),
    )
      .toEqual(new Set(["disp-1", "disp-2", "disp-3"]));
  });

  it("restores multi-row layout order with text and variable placement", () => {
    // Labeled-fixture shape with semantic ids: an entry_query source with
    // variable-bound filters, a saved_sql source with variable bindings, a
    // summary_title text block, and required variable controls placed in
    // their own row ahead of two component rows.
    const document: CompositionStudioDocument = {
      format: "ugoite.composition",
      format_version: 1,
      kind: "dashboard",
      name: "Monthly expenses",
      tags: [],
      spec: {
        variables: [
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
                  value: { variable: "month_start" },
                },
                {
                  field_id: 100,
                  operator: "lt",
                  value: { variable: "month_end" },
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
              month_start: { variable: "month_start" },
              month_end: { variable: "month_end" },
            },
          },
        ],
        components: [
          {
            kind: "text",
            id: "summary_title",
            content: { kind: "literal", text: "Monthly summary" },
            style: "heading",
          },
          {
            kind: "metric",
            id: "total",
            label: "Monthly total",
            source: "month_total",
            column: "total",
          },
          {
            kind: "table",
            id: "transactions",
            label: "Expense transactions",
            source: "expense_rows",
          },
          {
            kind: "input",
            id: "input-month_start",
            label: "Beginning",
            variable: "month_start",
          },
          {
            kind: "input",
            id: "input-month_start-secondary",
            label: "Start again",
            variable: "month_start",
          },
          {
            kind: "input",
            id: "input-month_end",
            variable: "month_end",
          },
        ],
        layout: {
          kind: "flow",
          rows: [
            {
              id: "controls",
              items: [
                { kind: "component", component: "input-month_start" },
                {
                  kind: "component",
                  component: "input-month_start-secondary",
                },
                { kind: "component", component: "input-month_end" },
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
    let draft = draftFromDocument(document, {
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
      {
        kind: "component",
        draftId: "input-month_start",
      },
      {
        kind: "component",
        draftId: "input-month_start-secondary",
      },
      {
        kind: "component",
        draftId: "input-month_end",
      },
    ]);
    expect(draft.inputs).toEqual([
      {
        draftId: "input-month_start",
        variable: "month_start",
        label: "Beginning",
      },
      {
        draftId: "input-month_start-secondary",
        variable: "month_start",
        label: "Start again",
      },
      { draftId: "input-month_end", variable: "month_end" },
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
      content: { kind: "literal", text: "Monthly summary" },
      style: "heading",
    });
    expect(draft.displays[1]).toMatchObject({
      kind: "metric",
      draftId: "disp-2",
      sourceDraftId: "src-2",
      column: "total",
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
    // Variable-bound entry_query filters round-trip verbatim.
    expect(toStudioDocument(draft).spec.sources[0]).toMatchObject({
      kind: "entry_query",
      query: {
        filters: [
          {
            field_id: 100,
            operator: "gte",
            value: { variable: "month_start" },
          },
          { field_id: 100, operator: "lt", value: { variable: "month_end" } },
        ],
      },
    });
    // Persisted definitions do not contain SQL parameter schemas. The draft
    // stays blocked until the exact pinned revision is read and typed.
    expect(draftSaveReadiness(draft)).toEqual({
      ready: false,
      reason: "refs",
    });
    const hydrated = setSavedSqlVariableTypes(draft, "src-2", {
      month_start: "date",
      month_end: "date",
    });
    expect(hydrated.ok).toBe(true);
    if (!hydrated.ok) throw new Error("expected exact revision schema");
    draft = hydrated.draft;
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
        expect(emittedById.has(item.component)).toBe(true);
      }
    }
    expect(
      emitted.spec.layout.rows.flatMap((row) => row.items),
    ).toEqual([
      { kind: "component", component: "input-month_start" },
      { kind: "component", component: "input-month_start-secondary" },
      { kind: "component", component: "input-month_end" },
      { kind: "component", component: "disp-1" },
      { kind: "component", component: "disp-2" },
      { kind: "component", component: "disp-3" },
    ]);
    expect(emitted.spec.components).toContainEqual({
      kind: "input",
      id: "input-month_start",
      label: "Beginning",
      variable: "month_start",
    });
    expect(emitted.spec.components).toContainEqual({
      kind: "input",
      id: "input-month_start-secondary",
      label: "Start again",
      variable: "month_start",
    });
    expect(emitted.spec.components).toContainEqual({
      kind: "text",
      id: "disp-1",
      content: { kind: "literal", text: "Monthly summary" },
      style: "heading",
    });
    expect(emitted.spec.components).toContainEqual({
      kind: "metric",
      id: "disp-2",
      label: "Monthly total",
      source: "src-2",
      column: "total",
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
        variables: [],
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

  it("round-trips variable-bound Text and keeps it through style edits", () => {
    let draft = createEmptyDraft("Variable text");
    const variable = addVariable(draft, {
      id: "greeting",
      type: "string",
      required: false,
    });
    if (!variable.ok) throw new Error("expected variable");
    draft = variable.draft;
    const text = addTextDisplay(draft, { text: "Hello" });
    if (!text.ok) throw new Error("expected text");

    const document = toStudioDocument(text.draft);
    const literal = document.spec.components.find((component) =>
      component.kind === "text"
    );
    if (!literal || literal.kind !== "text") {
      throw new Error("expected text component");
    }
    literal.content = { kind: "variable", variable: "greeting" };

    const restored = draftFromDocument(document, {});
    expect(restored.displays[0]).toMatchObject({
      kind: "text",
      content: { kind: "variable", variable: "greeting" },
    });
    expect(
      toStudioDocument(restored).spec.components.find((component) =>
        component.kind === "text"
      ),
    ).toMatchObject({
      kind: "text",
      content: { kind: "variable", variable: "greeting" },
    });

    const styled = setTextStyle(restored, "disp-1", "caption");
    expect(styled.ok).toBe(true);
    if (!styled.ok) throw new Error("expected text style update");
    expect(toStudioDocument(styled.draft).spec.components[0]).toMatchObject({
      kind: "text",
      content: { kind: "variable", variable: "greeting" },
      style: "caption",
    });
  });

  it("removes and retargets one Input without changing sibling Inputs", () => {
    let draft = createEmptyDraft("Repeated Input");
    const month = addVariable(draft, {
      id: "month",
      type: "date",
      required: false,
    });
    if (!month.ok) throw new Error("expected month");
    draft = month.draft;
    const region = addVariable(draft, {
      id: "region",
      type: "string",
      required: false,
    });
    if (!region.ok) throw new Error("expected region");
    draft = region.draft;
    const first = addInputComponent(draft, "month");
    if (!first.ok) throw new Error("expected first Input");
    const second = addInputComponent(first.draft, "month");
    if (!second.ok) throw new Error("expected second Input");
    const labeled = setInputLabel(second.draft, "input-2", "Secondary month");
    if (!labeled.ok) throw new Error("expected Input label");
    draft = labeled.draft;

    const retargeted = setInputVariable(draft, "input-2", "region");
    expect(retargeted.ok).toBe(true);
    if (!retargeted.ok) throw new Error("expected targeted retarget");
    expect(retargeted.draft.layoutRows[0].items).toEqual([
      { kind: "component", draftId: "input-1" },
      { kind: "component", draftId: "input-2" },
    ]);

    const removed = removeInputComponent(retargeted.draft, "input-1");
    expect(removed.ok).toBe(true);
    if (!removed.ok) throw new Error("expected targeted removal");
    expect(removed.draft.layoutRows[0].items).toEqual([
      { kind: "component", draftId: "input-2" },
    ]);
    expect(toStudioDocument(removed.draft).spec.components).toContainEqual({
      kind: "input",
      id: "input-2",
      label: "Secondary month",
      variable: "region",
    });
    expect(toStudioDocument(removed.draft).spec.components).not
      .toContainEqual({ kind: "input", id: "input-1", variable: "month" });
  });

  it("rejects removed Metric calculation shapes and unknown component/layout kinds", () => {
    const base = (): CompositionStudioDocument => ({
      format: "ugoite.composition",
      format_version: 1,
      kind: "dashboard",
      name: "Monthly expenses",
      tags: [],
      spec: {
        variables: [{ id: "month", type: "date", required: false }],
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
            column: "total",
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

    const legacyAggregate = base();
    (legacyAggregate.spec as { sources: unknown }).sources = [{
      kind: "entry_query",
      id: "entries",
      form_id: "11111111-1111-4111-8111-111111111111",
      field_schema: [{ field_id: 12, field_type: "integer" }],
      query: { filters: [], sort: [], projection: { kind: "preview" } },
    }];
    (legacyAggregate.spec as { components: unknown }).components = [{
      kind: "metric",
      id: "entry-total",
      definition: {
        kind: "aggregate",
        source: "entries",
        operation: "sum",
        field_id: 12,
      },
    }];
    expect(() => draftFromDocument(legacyAggregate, {})).toThrow();

    const legacyValueField = base();
    (legacyValueField.spec as { components: unknown }).components = [{
      kind: "metric",
      id: "total",
      source: "month_total",
      value_field: "total",
    }];
    expect(() => draftFromDocument(legacyValueField, {})).toThrow();

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
          content: { kind: "literal", text: "Monthly summary" },
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
          content: { kind: "literal", text: 42 as never },
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
          items: [{ kind: "variable", variable: "missing" }],
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

  it("places multiple Input components for one variable in selected rows", () => {
    let draft = createEmptyDraft();
    draft = addSavedSqlSource(draft, sqlSeed()).draft;
    const month = addVariable(draft, {
      id: "month",
      type: "date",
      required: true,
    });
    if (!month.ok) throw new Error("expected variable");
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
    const placed = addInputComponent(draft, "month", {
      rowId: headingRow.id,
      rowIndex: 0,
      itemIndex: 0,
    });
    expect(placed.ok).toBe(true);
    if (!placed.ok) throw new Error("expected variable placement");
    const inputId = placed.draftId!;
    draft = placed.draft;

    const document = toStudioDocument(draft);
    expect(document.spec.components).toContainEqual({
      kind: "text",
      id: "disp-1",
      content: { kind: "literal", text: "Summary" },
      style: "heading",
    });
    expect(document.spec.components).toContainEqual({
      kind: "input",
      id: inputId,
      variable: "month",
    });
    // Every component lands exactly once; the control references semantics.
    const items = document.spec.layout.rows.flatMap((row) => row.items);
    expect(items).toContainEqual({ kind: "component", component: "disp-1" });
    expect(items).toContainEqual({ kind: "component", component: "disp-2" });
    expect(items).toContainEqual({
      kind: "component",
      component: inputId,
    });
    // An Input is an ordinary component; adding another creates a second
    // stable component for the same variable.
    const repeated = addInputComponent(draft, "month");
    expect(repeated.ok).toBe(true);
    if (!repeated.ok) throw new Error("expected repeated Input");
    expect(repeated.draft.inputs.map((input) => input.variable)).toEqual([
      "month",
      "month",
    ]);
    expect(addInputComponent(draft, "nope")).toEqual({
      ok: false,
      error: "unknown-variable",
    });

    // Removing one Input leaves its variable and sibling Input intact.
    const removed = removeInputComponent(repeated.draft, inputId);
    expect(removed.ok).toBe(true);
    if (!removed.ok) throw new Error("expected Input removal");
    expect(removed.draft.variables.map((variable) => variable.id)).toEqual([
      "month",
    ]);
    expect(
      toStudioDocument(removed.draft).spec.layout.rows.flatMap((row) =>
        row.items
      ),
    ).not.toContainEqual({
      kind: "component",
      component: inputId,
    });
    expect(removed.draft.inputs).toHaveLength(1);
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
        item.kind === "component" ? item.component : item.variable
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

  it("blocks variable removal while a control is placed", () => {
    let draft = createEmptyDraft();
    const month = addVariable(draft, {
      id: "month",
      type: "date",
      required: true,
    });
    if (!month.ok) throw new Error("expected variable");
    draft = month.draft;
    const placed = addInputComponent(draft, "month");
    expect(placed.ok).toBe(true);
    if (!placed.ok) throw new Error("expected placement");
    expect(removeVariable(placed.draft, "month")).toEqual({
      ok: false,
      error: "variable-referenced",
    });
    const unplaced = removeInputComponent(placed.draft, placed.draftId!);
    if (!unplaced.ok) throw new Error("expected unplacement");
    expect(removeVariable(unplaced.draft, "month").ok).toBe(true);
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

  it("retargets a Metric between Saved SQL sources and requires a new column", () => {
    let draft = createEmptyDraft();
    draft = addSavedSqlSource(draft, sqlSeed()).draft;
    draft = addSavedSqlSource(draft, {
      ...sqlSeed(),
      entryId: "sql-2",
      name: "Monthly counts",
      expectedResult: [{ name: "count", type: "integer" }],
    }).draft;
    draft = addEntryQuerySource(draft, entrySeed()).draft;
    const metric = addMetricDisplay(
      draft,
      "src-1",
      "total",
      "Total",
    );
    if (!metric.ok) throw new Error("expected metric");
    draft = metric.draft;

    const moved = setMetricSource(draft, "disp-1", "src-2");
    expect(moved.ok).toBe(true);
    if (!moved.ok) throw new Error("expected source change");
    expect(
      toStudioDocument(moved.draft).spec.components[0],
    ).toMatchObject({
      kind: "metric",
      source: "src-2",
      column: "",
    });

    const rebound = setMetricColumn(moved.draft, "disp-1", "count");
    expect(rebound.ok).toBe(true);
    if (!rebound.ok) throw new Error("expected value change");
    expect(
      toStudioDocument(rebound.draft).spec.components[0],
    ).toMatchObject({
      source: "src-2",
      column: "count",
    });

    // Metrics remain SQL-backed; unknown identities fail closed.
    expect(setMetricSource(draft, "disp-1", "src-3")).toEqual({
      ok: false,
      error: "invalid-query",
    });
    expect(setMetricSource(draft, "disp-1", "src-9"))
      .toEqual({ ok: false, error: "unknown-source" });
    expect(setMetricSource(draft, "nope", "src-1"))
      .toEqual({ ok: false, error: "unknown-display" });
    expect(setMetricColumn(draft, "nope", "total")).toEqual({
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
      content: { kind: "literal", text: "New heading" },
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

  it("retargets variable controls in place without touching declarations", () => {
    let draft = createEmptyDraft();
    const month = addVariable(draft, {
      id: "month",
      label: "Month",
      type: "date",
      required: true,
    });
    if (!month.ok) throw new Error("expected month");
    draft = month.draft;
    const region = addVariable(draft, {
      id: "region",
      label: "Region",
      type: "string",
      required: false,
    });
    if (!region.ok) throw new Error("expected region");
    draft = region.draft;
    const quarter = addVariable(draft, {
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
    const placed = addInputComponent(draft, "month");
    if (!placed.ok) throw new Error("expected placement");
    const inputId = placed.draftId!;
    draft = placed.draft;
    const before = draft.layoutRows.map((row) => ({
      ...row,
      items: [...row.items],
    }));

    const retargeted = setInputVariable(draft, inputId, "region");
    expect(retargeted.ok).toBe(true);
    if (!retargeted.ok) throw new Error("expected retarget");
    const items = toStudioDocument(retargeted.draft).spec.layout.rows.flatMap(
      (row) => row.items,
    );
    expect(items).toContainEqual({
      kind: "component",
      component: inputId,
    });
    expect(items).not.toContainEqual({
      kind: "component",
      component: "input-region",
    });
    expect(toStudioDocument(retargeted.draft).spec.components).toContainEqual({
      kind: "input",
      id: inputId,
      variable: "region",
    });
    // Same row, same position; declarations untouched with no label override.
    expect(retargeted.draft.layoutRows.map((row) => row.id)).toEqual(
      before.map((row) => row.id),
    );
    expect(
      retargeted.draft.variables.map((variable) => variable.id),
    ).toEqual(["month", "region", "quarter"]);

    expect(setInputVariable(draft, inputId, "month"))
      .toEqual({
        ok: true,
        draft,
      });
    expect(setInputVariable(draft, inputId, "nope"))
      .toEqual({
        ok: false,
        error: "unknown-variable",
      });
    expect(setInputVariable(draft, "missing-input", "month"))
      .toEqual({ ok: false, error: "unknown-input" });
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
    // Variable-bound values keep their shape through the same channel.
    const bound = setEntryQueryFilters(draft, "src-1", [
      { field_id: 1, operator: "gte", value: { variable: "month" } },
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

  it("stores selected timestamps separately from EntryQuery projection fields", () => {
    let draft = createEmptyDraft();
    draft = addEntryQuerySource(draft, {
      ...entrySeed(),
      query: {
        ...entrySeed().query,
        projection: { kind: "fields", fields: [1] },
      },
    }).draft;
    const updated = setEntryQueryDisplaySystemFields(draft, "src-1", [
      "updated_at",
      "created_at",
    ]);
    expect(updated.ok).toBe(true);
    if (!updated.ok) throw new Error("expected timestamp selection");

    const document = toStudioDocument(updated.draft);
    expect(document.spec.sources[0]).toMatchObject({
      kind: "entry_query",
      query: {
        projection: { kind: "fields", fields: [1] },
        display_system_fields: ["created_at", "updated_at"],
      },
    });
    const restored = draftFromDocument(document, {});
    expect(toStudioDocument(restored).spec.sources[0]).toMatchObject({
      kind: "entry_query",
      query: {
        projection: { kind: "fields", fields: [1] },
        display_system_fields: ["created_at", "updated_at"],
      },
    });
  });

  it("keeps a Saved SQL Metric independent from EntryQuery table projection", () => {
    let draft = createEmptyDraft();
    draft = addEntryQuerySource(draft, {
      ...entrySeed(),
      fieldSchema: [
        { field_id: 1, field_type: "string" },
        { field_id: 2, field_type: "integer" },
      ],
    }).draft;
    draft = addSavedSqlSource(draft, {
      ...sqlSeed(),
      expectedResult: [{ name: "total", type: "integer" }],
    }).draft;
    const metric = addMetricDisplay(draft, "src-2", "total");
    const table = addTableDisplay(metric.ok ? metric.draft : draft, "src-1");
    expect(metric.ok).toBe(true);
    expect(table.ok).toBe(true);
    if (!metric.ok || !table.ok) throw new Error("expected metric and table");
    draft = table.draft;
    expect(toStudioDocument(draft).spec.sources[0]).toMatchObject({
      kind: "entry_query",
      query: { projection: { kind: "preview" } },
    });

    const preview = setEntryQueryProjection(draft, "src-1", {
      kind: "preview",
    });
    expect(preview.ok).toBe(true);
    if (!preview.ok) throw new Error("expected projection change");
    expect(toStudioDocument(preview.draft).spec.sources[0]).toMatchObject({
      query: { projection: { kind: "preview" } },
    });
    expect(toStudioDocument(preview.draft).spec.components[0]).toMatchObject({
      kind: "metric",
      source: "src-2",
      column: "total",
    });
  });

  it("points saved sql sources at an exact revision preserving bindings", () => {
    let draft = createEmptyDraft();
    const period = addVariable(draft, {
      id: "period",
      type: "date",
      required: true,
    });
    if (!period.ok) throw new Error("expected period variable");
    draft = period.draft;
    const region = addVariable(draft, {
      id: "region",
      type: "string",
      required: false,
    });
    if (!region.ok) throw new Error("expected region variable");
    draft = region.draft;
    draft = addSavedSqlSource(draft, {
      ...sqlSeed(),
      variables: {
        month: { variable: "period" },
        gone: { variable: "gone" },
      },
    }).draft;
    const updated = setSavedSqlRevision(draft, "src-1", {
      revisionId: "rev-2",
      expectedResult: [{ name: "total", type: "float" }],
      variableTypes: { month: "date", region: "string" },
    });
    expect(updated.ok).toBe(true);
    if (!updated.ok) throw new Error("expected revision update");
    const source = updated.draft.sources[0];
    if (source.kind !== "saved_sql") throw new Error("expected saved sql");
    expect(source.revisionId).toBe("rev-2");
    // Existing exact-type bindings survive; new same-name bindings are not
    // guessed, and bindings removed by the exact revision disappear.
    expect(source.variables).toEqual({
      month: { variable: "period" },
    });
    expect(source.variableTypes).toEqual({ month: "date", region: "string" });
    expect(updated.draft.sources[0]).toMatchObject({
      kind: "saved_sql",
      revisionId: "rev-2",
    });
    expect(
      setSavedSqlRevision(draft, "src-1", {
        revisionId: "  ",
        expectedResult: [],
        variableTypes: {},
      }),
    ).toEqual({ ok: false, error: "invalid-query" });
    expect(
      setSavedSqlRevision(draft, "nope", {
        revisionId: "rev-2",
        expectedResult: [],
        variableTypes: {},
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

  it("seeds the exact Saved SQL revision without promoting run values to defaults", () => {
    const { draft, draftId } = applyStudioSeed(createEmptyDraft(), {
      kind: "saved_sql",
      seed: {
        entryId: "sql-1",
        revisionId: "rev-1",
        name: "Monthly",
        expectedResult: [{ name: "total", type: "float" as const }],
        variables: {},
        variableTypes: { month_start: "date", month_end: "date" },
      },
    });
    expect(draftId).toBe("src-1");
    expect(draft.name).toBe("Monthly");
    const source = draft.sources[0];
    if (source.kind !== "saved_sql") throw new Error("expected saved sql");
    expect(source.revisionId).toBe("rev-1");
    // Source variables do not silently create Composition variables or
    // inherit transient values entered on the SQL run page.
    expect(draft.variables).toEqual([]);
    expect(draft.inputs).toEqual([]);
    const items = draft.layoutRows.flatMap((row) => row.items);
    expect(items).toEqual([
      { kind: "component", draftId: "disp-1" },
    ]);
    expect(draft.displays[0]).toMatchObject({
      kind: "table",
      sourceDraftId: "src-1",
      label: "Monthly",
    });
    expect(draftSaveReadiness(draft)).toEqual({
      ready: false,
      reason: "refs",
    });

    const declared = addVariable(draft, {
      id: "period_start",
      type: "date",
      required: true,
      default: "2026-01-01",
    });
    if (!declared.ok) throw new Error("expected explicit variable");
    const bound = setSavedSqlVariableBinding(
      declared.draft,
      "src-1",
      "month_start",
      "period_start",
    );
    expect(bound.ok).toBe(true);
    if (!bound.ok) throw new Error("expected explicit source binding");
    const input = addInputComponent(bound.draft, "period_start");
    expect(input.ok).toBe(true);
    if (!input.ok) throw new Error("expected explicit Input");
    const periodEnd = addVariable(input.draft, {
      id: "period_end",
      type: "date",
      required: true,
    });
    if (!periodEnd.ok) throw new Error("expected end variable");
    const boundEnd = setSavedSqlVariableBinding(
      periodEnd.draft,
      "src-1",
      "month_end",
      "period_end",
    );
    expect(boundEnd.ok).toBe(true);
    if (!boundEnd.ok) throw new Error("expected explicit second binding");
    const inputEnd = addInputComponent(boundEnd.draft, "period_end");
    expect(inputEnd.ok).toBe(true);
    if (!inputEnd.ok) throw new Error("expected second explicit Input");
    expect(draftSaveReadiness(inputEnd.draft)).toEqual({ ready: true });
  });

  it("seeds saved sql without variables with a table and no variable controls", () => {
    const { draft } = applyStudioSeed(createEmptyDraft(), {
      kind: "saved_sql",
      seed: sqlSeed(),
    });
    expect(draft.variables).toHaveLength(0);
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

    // Required variables without a default need a placed control.
    const month = addVariable(draft, {
      id: "month",
      type: "date",
      required: true,
    });
    if (!month.ok) throw new Error("expected variable");
    expect(draftSaveReadiness(month.draft)).toEqual({
      ready: false,
      reason: "refs",
    });
    const placed = addInputComponent(month.draft, "month");
    if (!placed.ok) throw new Error("expected variable placement");
    expect(draftSaveReadiness(placed.draft)).toEqual({ ready: true });
  });

  it("blocks saves when a retargeted Metric has no selected SQL column", () => {
    let sqlDraft = addSavedSqlSource(createEmptyDraft("Monthly"), sqlSeed())
      .draft;
    sqlDraft = addSavedSqlSource(sqlDraft, {
      ...sqlSeed(),
      entryId: "sql-2",
      name: "Monthly count",
      expectedResult: [{ name: "count", type: "integer" }],
    }).draft;
    const scalar = addMetricDisplay(sqlDraft, "src-1", "total");
    if (!scalar.ok || !scalar.draftId) {
      throw new Error("expected scalar metric");
    }
    sqlDraft = scalar.draft;
    expect(draftSaveReadiness(sqlDraft)).toEqual({ ready: true });

    const moved = setMetricSource(sqlDraft, scalar.draftId, "src-2");
    if (!moved.ok) throw new Error("expected metric source change");
    expect(draftSaveReadiness(moved.draft)).toEqual({
      ready: false,
      reason: "metric",
    });

    const selected = setMetricColumn(moved.draft, scalar.draftId, "count");
    if (!selected.ok) throw new Error("expected result column selection");
    expect(draftSaveReadiness(selected.draft)).toEqual({ ready: true });

    const missingSqlColumn = setMetricColumn(
      selected.draft,
      scalar.draftId,
      undefined,
    );
    if (!missingSqlColumn.ok) {
      throw new Error("expected incomplete SQL scalar to remain editable");
    }
    expect(draftSaveReadiness(missingSqlColumn.draft)).toEqual({
      ready: false,
      reason: "metric",
    });
  });
});
