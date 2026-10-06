import { describe, expect, it } from "vitest";
import {
  addEntryQuerySource,
  addMetricDisplay,
  addParameter,
  addSavedSqlSource,
  addTableDisplay,
  addTextDisplay,
  type CompositionDraft,
  createEmptyDraft,
  placeParameterControl,
  removeDisplay,
} from "./composition-draft";
import {
  blockIdsUsingSource,
  sourceDraftIdForBlock,
  STUDIO_MODES,
  visibleComponentSourceIds,
} from "./composition-studio-sync";

const sqlSeed = (name: string) => ({
  entryId: `sql-${name}`,
  revisionId: "rev-1",
  name,
  expectedResult: [{ name: "total", type: "float" as const }],
  variables: {},
});

const entrySeed = (name: string) => ({
  formId: "11111111-1111-4111-8111-111111111111",
  name,
  fieldSchema: [{ field_id: 1, field_type: "string" }],
  query: {
    filters: [],
    sort: [],
    projection: { kind: "preview" as const },
  },
});

/** Metric on src-1, table on src-2, text, placed month parameter. */
const seedDraft = (): CompositionDraft => {
  let draft = createEmptyDraft("Studio");
  draft = addSavedSqlSource(draft, sqlSeed("Monthly")).draft;
  draft = addEntryQuerySource(draft, entrySeed("Expenses")).draft;
  const month = addParameter(draft, {
    id: "month",
    label: "Month",
    type: "date",
    required: true,
  });
  if (!month.ok) throw new Error("expected month parameter");
  draft = month.draft;
  const metric = addMetricDisplay(
    draft,
    "src-1",
    { column: "total" },
    "Total",
  );
  if (!metric.ok) throw new Error("expected metric block");
  draft = metric.draft;
  const table = addTableDisplay(draft, "src-2", "Details");
  if (!table.ok) throw new Error("expected table block");
  draft = table.draft;
  const text = addTextDisplay(draft, { text: "Summary", style: "heading" });
  if (!text.ok) throw new Error("expected text block");
  draft = text.draft;
  const placed = placeParameterControl(draft, "month");
  if (!placed.ok) throw new Error("expected parameter placement");
  return placed.draft;
};

describe("composition studio sync", () => {
  it("exposes the Design, Data, and Split workspace modes", () => {
    expect([...STUDIO_MODES]).toEqual(["design", "data", "split"]);
  });

  it("maps metric and table blocks to their component source", () => {
    const draft = seedDraft();
    expect(sourceDraftIdForBlock(draft, "disp-1")).toBe("src-1");
    expect(sourceDraftIdForBlock(draft, "disp-2")).toBe("src-2");
  });

  it("keeps the Data selection for text, parameter, and unknown blocks", () => {
    const draft = seedDraft();
    expect(sourceDraftIdForBlock(draft, "disp-3")).toBeNull();
    expect(sourceDraftIdForBlock(draft, "param:month")).toBeNull();
    expect(sourceDraftIdForBlock(draft, null)).toBeNull();
    expect(sourceDraftIdForBlock(draft, "disp-999")).toBeNull();
  });

  it("maps a source to the component blocks that use it", () => {
    const draft = seedDraft();
    expect(blockIdsUsingSource(draft, "src-1")).toEqual(["disp-1"]);
    expect(blockIdsUsingSource(draft, "src-2")).toEqual(["disp-2"]);
    expect(blockIdsUsingSource(draft, "src-999")).toEqual([]);
    expect(blockIdsUsingSource(draft, null)).toEqual([]);
  });

  it("lists visible-component sources without text or unplaced blocks", () => {
    const draft = seedDraft();
    expect(visibleComponentSourceIds(draft)).toEqual(["src-1", "src-2"]);

    const removed = removeDisplay(draft, "disp-1");
    if (!removed.ok) throw new Error("expected table removal");
    expect(visibleComponentSourceIds(removed.draft)).toEqual(["src-2"]);
    expect(blockIdsUsingSource(removed.draft, "src-1")).toEqual([]);
    expect(sourceDraftIdForBlock(removed.draft, "disp-1")).toBeNull();
  });
});
