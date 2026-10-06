import { describe, expect, it, vi } from "vitest";
import {
  addEntryQuerySource,
  addMetricDisplay,
  addParameter,
  addSavedSqlSource,
  addTableDisplay,
  canonicalizeDraft,
  type CompositionStudioDocument,
  createEmptyDraft,
  defaultParameterValues,
  displaysUsingSource,
  draftFromDocument,
  ensureParametersForVariables,
  moveDisplay,
  moveSource,
  removeDisplay,
  removeParameter,
  removeSource,
  setDraftName,
  setDraftTags,
  studioSeedState,
  toStudioDocument,
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
});
