import { describe, expect, it, vi } from "vitest";
import {
  addInlineSqlSource,
  addInputComponent,
  addMetricDisplay,
  addSavedSqlSource,
  addSqlArgumentVariables,
  addTableDisplay,
  addVariable,
  canonicalizeDraft,
  createEmptyDraft,
  defaultVariableValues,
  draftFromDocument,
  removeSource,
  setInlineSql,
  setInlineSqlDescriptor,
  setInlineSqlName,
  setMetricSource,
  toStudioDocument,
} from "./composition-draft";

const { canonicalizeDocumentMock } = vi.hoisted(() => ({
  canonicalizeDocumentMock: vi.fn(async (document: unknown) => ({
    document,
    canonical_yaml: "canonical",
    fingerprint: "fp",
  })),
}));

vi.mock("./ugoite-client", () => ({
  canonicalizeCompositionDocument: (...args: unknown[]) =>
    canonicalizeDocumentMock(...args),
}));

const savedSqlSeed = (overrides: Record<string, unknown> = {}) => ({
  entryId: "sql-entry",
  revisionId: "sql-rev-7",
  name: "Monthly totals",
  expectedResult: [{ name: "total", type: "decimal" as const }],
  variables: {},
  variableTypes: {},
  ...overrides,
});

const inlineSeed = () => ({
  name: "Expenses",
  sql: 'SELECT "Amount" AS total FROM "Expenses"',
});

describe("SQL-first Composition draft", () => {
  it("adds typed SQL arguments together with placed Input components", () => {
    const result = addSqlArgumentVariables(createEmptyDraft("Report"), [
      { name: "filter_amount", type: "integer" },
      { name: "filter_active", type: "boolean" },
    ]);

    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.draft.variables).toEqual([
      { id: "filter_amount", type: "integer", required: true },
      { id: "filter_active", type: "boolean", required: true },
    ]);
    expect(result.draft.inputs).toEqual([
      { draftId: "input-1", variable: "filter_amount" },
      { draftId: "input-2", variable: "filter_active" },
    ]);
    expect(result.draft.layoutRows[0]?.items).toEqual([
      { kind: "component", draftId: "input-1" },
      { kind: "component", draftId: "input-2" },
    ]);
  });

  it("rejects SQL argument creation when a variable ID has a different type", () => {
    const draft = {
      ...createEmptyDraft("Report"),
      variables: [{
        id: "filter_amount",
        type: "string" as const,
        required: true,
      }],
    };

    expect(addSqlArgumentVariables(draft, [
      { name: "filter_new", type: "integer" },
      { name: "filter_amount", type: "integer" },
    ])).toEqual({ ok: false, error: "invalid-query" });
    expect(draft.variables).toEqual([
      { id: "filter_amount", type: "string", required: true },
    ]);
    expect(draft.inputs).toEqual([]);
  });

  it("copies Saved SQL defaults into Composition-owned variables once", () => {
    const added = addSavedSqlSource(
      createEmptyDraft("Report"),
      savedSqlSeed({
        variableTypes: { start: "date", region: "string" },
        variableDefaults: { start: "2026-10-01" },
      }),
    );

    expect(added.draft.variables).toEqual([
      { id: "start", type: "date", required: false, default: "2026-10-01" },
      { id: "region", type: "string", required: true },
    ]);
    expect(added.draft.inputs).toEqual([
      { draftId: "input-1", variable: "region" },
    ]);
    expect(added.draft.sources[0]).toMatchObject({
      kind: "saved_sql",
      revisionId: "sql-rev-7",
      variables: {
        start: { variable: "start" },
        region: { variable: "region" },
      },
    });
    expect(defaultVariableValues(added.draft)).toEqual({ start: "2026-10-01" });
  });

  it("keeps an existing Composition default when binding a Saved SQL variable", () => {
    const added = addSavedSqlSource(
      {
        ...createEmptyDraft("Report"),
        variables: [{
          id: "region",
          type: "string",
          required: false,
          default: "West",
        }],
      },
      savedSqlSeed({
        variableTypes: { region: "string" },
        variableDefaults: { region: "East" },
      }),
    );

    expect(added.draft.variables).toEqual([{
      id: "region",
      type: "string",
      required: false,
      default: "West",
    }]);
    expect(added.draft.sources[0]).toMatchObject({
      kind: "saved_sql",
      variables: { region: { variable: "region" } },
    });
  });

  it("turns a Form seed into editable inline SQL without an EntryQuery source", () => {
    const added = addInlineSqlSource(createEmptyDraft(), {
      name: "Expenses",
      sql: 'SELECT * FROM "Expenses"',
    });
    const document = toStudioDocument(added.draft);

    expect(document.spec.sources).toEqual([{
      kind: "inline_sql",
      id: "src-1",
      name: "Expenses",
      sql: 'SELECT * FROM "Expenses"',
      form_bindings: [],
      expected_result: [],
      schema_fingerprint: "",
    }]);
    expect(JSON.stringify(document)).not.toContain("entry_query");
  });

  it("clears server descriptors after SQL edits and preserves them on rename", () => {
    let draft = addInlineSqlSource(createEmptyDraft(), inlineSeed()).draft;
    const descriptor = {
      formBindings: [{ relation: "Expenses", form_id: "form-1" }],
      expectedResult: [{ name: "total", type: "decimal" as const }],
      schemaFingerprint: "a".repeat(64),
    };
    const pinned = setInlineSqlDescriptor(draft, "src-1", descriptor);
    if (!pinned.ok) throw new Error("expected descriptor update");
    draft = pinned.draft;

    const renamed = setInlineSqlName(draft, "src-1", "Cost detail");
    if (!renamed.ok) throw new Error("expected source rename");
    expect(renamed.draft.sources[0]).toMatchObject({
      name: "Cost detail",
      ...descriptor,
    });

    const edited = setInlineSql(
      renamed.draft,
      "src-1",
      'SELECT "Amount" FROM "Expenses"',
    );
    if (!edited.ok) throw new Error("expected SQL edit");
    expect(edited.draft.sources[0]).toMatchObject({
      sql: 'SELECT "Amount" FROM "Expenses"',
      formBindings: [],
      expectedResult: [],
      schemaFingerprint: "",
    });
  });

  it("pins the authorized result descriptor idempotently", () => {
    const draft = addInlineSqlSource(createEmptyDraft(), inlineSeed()).draft;
    const descriptor = {
      formBindings: [{ relation: "Expenses", form_id: "form-1" }],
      expectedResult: [{ name: "total", type: "decimal" as const }],
      schemaFingerprint: "b".repeat(64),
    };
    const once = setInlineSqlDescriptor(draft, "src-1", descriptor);
    if (!once.ok) throw new Error("expected descriptor update");
    const twice = setInlineSqlDescriptor(once.draft, "src-1", descriptor);
    if (!twice.ok) throw new Error("expected idempotent update");
    expect(twice.draft).toBe(once.draft);
    expect(setInlineSqlDescriptor(draft, "missing", descriptor)).toEqual({
      ok: false,
      error: "unknown-source",
    });
  });

  it("serializes only SQL source kinds and contains no persisted display labels or tags", () => {
    let draft = createEmptyDraft("Report");
    draft = addInlineSqlSource(draft, {
      ...inlineSeed(),
      formBindings: [{ relation: "Expenses", form_id: "form-1" }],
      expectedResult: [{ name: "total", type: "decimal" }],
      schemaFingerprint: "c".repeat(64),
    }).draft;
    draft = addSavedSqlSource(draft, savedSqlSeed()).draft;
    const table = addTableDisplay(draft, "src-1");
    if (!table.ok) throw new Error("expected table component");
    const inputVariable = addVariable(table.draft, {
      id: "month",
      type: "date",
      required: true,
    });
    if (!inputVariable.ok) throw new Error("expected variable");
    const input = addInputComponent(inputVariable.draft, "month");
    if (!input.ok) throw new Error("expected Input component");

    const document = toStudioDocument(input.draft);
    expect(document.spec.sources.map((source) => source.kind)).toEqual([
      "inline_sql",
      "saved_sql",
    ]);
    expect(document).not.toHaveProperty("tags");
    expect(JSON.stringify(document)).not.toMatch(/\"label\"\s*:/);
  });

  it("round-trips inline SQL metadata and the exact Saved SQL revision", () => {
    let draft = createEmptyDraft("Report");
    draft = addInlineSqlSource(draft, {
      ...inlineSeed(),
      formBindings: [{ relation: "Expenses", form_id: "form-1" }],
      expectedResult: [{ name: "total", type: "decimal" }],
      schemaFingerprint: "d".repeat(64),
    }).draft;
    draft = addSavedSqlSource(draft, savedSqlSeed()).draft;
    const table = addTableDisplay(draft, "src-1");
    if (!table.ok) throw new Error("expected table component");
    draft = table.draft;
    const metric = addMetricDisplay(draft, "src-2", "total");
    if (!metric.ok) throw new Error("expected metric component");

    const document = toStudioDocument(metric.draft);
    const restored = draftFromDocument(
      document,
      { "src-2": "Monthly totals" },
      {
        "src-2": "SELECT SUM(amount) AS total FROM expenses",
      },
    );
    expect(restored.sources).toMatchObject([
      {
        kind: "inline_sql",
        name: "Expenses",
        sql: 'SELECT "Amount" AS total FROM "Expenses"',
        formBindings: [{ relation: "Expenses", form_id: "form-1" }],
        schemaFingerprint: "d".repeat(64),
      },
      {
        kind: "saved_sql",
        entryId: "sql-entry",
        revisionId: "sql-rev-7",
        name: "Monthly totals",
        sql: "SELECT SUM(amount) AS total FROM expenses",
      },
    ]);
    expect(toStudioDocument(restored).spec.sources).toEqual(
      document.spec.sources,
    );
  });

  it("fails closed when asked to restore the replaced EntryQuery grammar", () => {
    const oldDocument = {
      format: "ugoite.composition",
      format_version: 1,
      kind: "dashboard",
      name: "Old",
      spec: {
        variables: [],
        sources: [{
          kind: "entry_query",
          id: "src-1",
          form_id: "form-1",
          query: { filters: [], sort: [], projection: { kind: "preview" } },
        }],
        components: [],
        layout: { kind: "flow", rows: [{ id: "row-1", items: [] }] },
      },
    } as never;

    expect(() => draftFromDocument(oldDocument, {}, {})).toThrow(
      "Unsupported Composition document source kind: entry_query",
    );
  });

  it("requires a new scalar column when a Metric moves between SQL sources", () => {
    let draft = addInlineSqlSource(createEmptyDraft(), {
      ...inlineSeed(),
      expectedResult: [{ name: "total", type: "decimal" }],
    }).draft;
    draft = addSavedSqlSource(
      draft,
      savedSqlSeed({
        expectedResult: [{ name: "count", type: "integer" }],
      }),
    ).draft;
    const metric = addMetricDisplay(draft, "src-1", "total");
    if (!metric.ok || !metric.draftId) throw new Error("expected metric");
    const changed = setMetricSource(metric.draft, metric.draftId, "src-2");
    if (!changed.ok) throw new Error("expected metric source change");
    expect(changed.draft.displays[0]).toMatchObject({
      kind: "metric",
      sourceDraftId: "src-2",
    });
    expect(changed.draft.displays[0]).not.toHaveProperty("column");
  });

  it("keeps source removal blocked while a display references it", () => {
    const source = addInlineSqlSource(createEmptyDraft(), inlineSeed());
    const withTable = addTableDisplay(source.draft, source.draftId);
    if (!withTable.ok) throw new Error("expected table component");
    expect(removeSource(withTable.draft, source.draftId)).toEqual({
      ok: false,
      error: "source-referenced",
    });
  });

  it("canonicalizes SQL-only source documents through the shared contract", async () => {
    const draft =
      addInlineSqlSource(createEmptyDraft("Report"), inlineSeed()).draft;
    const result = await canonicalizeDraft(draft);
    expect(result.canonical_yaml).toBe("canonical");
    expect(canonicalizeDocumentMock).toHaveBeenCalledWith(
      expect.objectContaining({
        spec: expect.objectContaining({
          sources: [expect.objectContaining({ kind: "inline_sql" })],
        }),
      }),
    );
  });
});
