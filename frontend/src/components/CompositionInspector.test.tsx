import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSignal } from "solid-js";
import {
  CompositionInspector,
  type CompositionInspectorDataJump,
} from "./CompositionInspector";
import { designBlockIdForParameter } from "./CompositionDesignCanvas";
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
  toStudioDocument,
} from "~/lib/composition-draft";
import { setLocale } from "~/lib/i18n";

const sqlSeed = (name: string) => ({
  entryId: `sql-${name}`,
  revisionId: "rev-1",
  name,
  expectedResult: [
    { name: "total", type: "float" as const },
    { name: "count", type: "integer" as const },
  ],
  variables: {},
});

const entrySeed = () => ({
  formId: "form-1",
  name: "Expenses",
  fieldSchema: [
    { field_id: 1, field_type: "string" },
    { field_id: 2, field_type: "double" },
  ],
  query: { filters: [], sort: [], projection: { kind: "preview" as const } },
});

/** Metric on sql-1, table on the entry source, text, month placed. */
const seedDraft = (): CompositionDraft => {
  let draft = createEmptyDraft("Studio");
  draft = addSavedSqlSource(draft, sqlSeed("Monthly")).draft;
  draft = addEntryQuerySource(draft, entrySeed()).draft;
  draft = addSavedSqlSource(draft, sqlSeed("Backup")).draft;
  const month = addParameter(draft, {
    id: "month",
    label: "Month",
    type: "date",
    required: true,
  });
  if (!month.ok) throw new Error("expected month parameter");
  draft = month.draft;
  const region = addParameter(draft, {
    id: "region",
    label: "Region",
    type: "string",
    required: false,
  });
  if (!region.ok) throw new Error("expected region parameter");
  draft = region.draft;
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

const harnessCalls = vi.hoisted(() => ({
  draft: [] as CompositionDraft[],
  jump: [] as CompositionInspectorDataJump[],
}));

function Harness(
  props: { initial: CompositionDraft; selectedId: string | null },
) {
  const [draft, setDraft] = createSignal(props.initial);
  return (
    <CompositionInspector
      draft={draft()}
      selectedId={props.selectedId}
      onDraftChange={(next) => {
        harnessCalls.draft.push(next);
        setDraft(next);
      }}
      onDataJump={(jump) => harnessCalls.jump.push(jump)}
    />
  );
}

const renderHarness = (
  selectedId: string | null,
  initial?: CompositionDraft,
) =>
  render(() => (
    <Harness initial={initial ?? seedDraft()} selectedId={selectedId} />
  ));

const documentSources = (draft: CompositionDraft): string =>
  JSON.stringify(toStudioDocument(draft).spec.sources);

const metricComponent = (draft: CompositionDraft, id: string) =>
  toStudioDocument(draft).spec.components.find((component) =>
    component.id === id
  );

describe("CompositionInspector", () => {
  beforeEach(() => {
    setLocale("en");
    vi.clearAllMocks();
    harnessCalls.draft.length = 0;
    harnessCalls.jump.length = 0;
  });

  afterEach(() => {
    cleanup();
  });

  it("binds metric label edits to the draft display", () => {
    const initial = seedDraft();
    const before = documentSources(initial);
    renderHarness("disp-1", initial);

    fireEvent.input(screen.getByLabelText("Label"), {
      target: { value: "Monthly total" },
    });

    expect(harnessCalls.draft).toHaveLength(1);
    const next = harnessCalls.draft[0];
    expect(metricComponent(next, "disp-1")).toMatchObject({
      kind: "metric",
      label: "Monthly total",
    });
    // Label edits never reshape sources, so they never refetch.
    expect(documentSources(next)).toBe(before);
    expect(harnessCalls.jump).toHaveLength(0);
  });

  it("shows Form labels instead of field IDs for metric values", () => {
    const added = addMetricDisplay(seedDraft(), "src-2", { fieldId: 1 });
    if (!added.ok || !added.draftId) throw new Error("expected metric block");

    render(() => (
      <CompositionInspector
        draft={added.draft}
        selectedId={added.draftId}
        fieldNames={(_formId, fieldId) =>
          fieldId === 1 ? "Expense type" : undefined}
        onDraftChange={() => {}}
        onDataJump={() => {}}
      />
    ));

    const valueSelect = screen.getByLabelText("Value");
    const options = Array.from(valueSelect.querySelectorAll("option")).map(
      (option) => option.textContent,
    );
    expect(options).toEqual(["Expense type", "Field 2"]);
    expect(options.join(" ")).not.toContain("1");
  });

  it("maps metric source changes to component source and value field", () => {
    renderHarness("disp-1");

    // The entry source shares no value field: first schema field wins.
    fireEvent.change(screen.getByLabelText("Source"), {
      target: { value: "src-2" },
    });
    expect(harnessCalls.draft).toHaveLength(1);
    expect(metricComponent(harnessCalls.draft[0], "disp-1")).toMatchObject({
      kind: "metric",
      source: "src-2",
      value_field: { kind: "entry_field", field_id: 1 },
    });

    // The backup SQL source still declares total: the value field is kept.
    fireEvent.change(screen.getByLabelText("Source"), {
      target: { value: "src-1" },
    });
    fireEvent.change(screen.getByLabelText("Source"), {
      target: { value: "src-3" },
    });
    const last = harnessCalls.draft[harnessCalls.draft.length - 1];
    expect(metricComponent(last, "disp-1")).toMatchObject({
      kind: "metric",
      source: "src-3",
      value_field: { kind: "sql_column", name: "total" },
    });
  });

  it("maps metric value changes to the typed value field", () => {
    renderHarness("disp-1");

    fireEvent.change(screen.getByLabelText("Value"), {
      target: { value: "count" },
    });

    expect(harnessCalls.draft).toHaveLength(1);
    const next = harnessCalls.draft[0];
    expect(metricComponent(next, "disp-1")).toMatchObject({
      kind: "metric",
      source: "src-1",
      value_field: { kind: "sql_column", name: "count" },
    });
  });

  it("binds table label and source edits to the draft display", () => {
    renderHarness("disp-2");

    fireEvent.input(screen.getByLabelText("Label"), {
      target: { value: "Lines" },
    });
    fireEvent.change(screen.getByLabelText("Source"), {
      target: { value: "src-1" },
    });

    expect(harnessCalls.draft).toHaveLength(2);
    const last = harnessCalls.draft[harnessCalls.draft.length - 1];
    const table = last.displays.find((display) => display.draftId === "disp-2");
    expect(table).toMatchObject({
      kind: "table",
      label: "Lines",
      sourceDraftId: "src-1",
    });
  });

  it("binds text content and style edits without touching sources", () => {
    const initial = seedDraft();
    const before = documentSources(initial);
    renderHarness("disp-3", initial);

    fireEvent.input(screen.getByRole("textbox", { name: "Text" }), {
      target: { value: "New heading" },
    });
    fireEvent.change(screen.getByLabelText("Style"), {
      target: { value: "title" },
    });

    expect(harnessCalls.draft).toHaveLength(2);
    const last = harnessCalls.draft[harnessCalls.draft.length - 1];
    const text = last.displays.find((display) => display.draftId === "disp-3");
    expect(text).toMatchObject({
      kind: "text",
      text: "New heading",
      style: "title",
    });
    // Text carries no source binding: sources stay stable, no refetch.
    expect(documentSources(last)).toBe(before);
    expect(harnessCalls.jump).toHaveLength(0);
  });

  it("localizes text-style labels without changing the stored style enum", () => {
    const initial = seedDraft();
    renderHarness("disp-3", initial);

    const styleSelect = screen.getByLabelText("Style") as HTMLSelectElement;
    const optionLabelsAndValues = () =>
      Array.from(styleSelect.options).map((option) => ({
        label: option.textContent,
        value: option.value,
      }));

    expect(optionLabelsAndValues()).toEqual([
      { label: "Title", value: "title" },
      { label: "Heading", value: "heading" },
      { label: "Body", value: "body" },
      { label: "Caption", value: "caption" },
    ]);

    setLocale("ja");
    expect(styleSelect.value).toBe("heading");
    expect(optionLabelsAndValues()).toEqual([
      { label: "タイトル", value: "title" },
      { label: "見出し", value: "heading" },
      { label: "本文", value: "body" },
      { label: "キャプション", value: "caption" },
    ]);

    fireEvent.change(styleSelect, { target: { value: "caption" } });
    expect(harnessCalls.draft).toHaveLength(1);
    expect(
      harnessCalls.draft[0].displays.find((display) =>
        display.draftId === "disp-3"
      ),
    ).toMatchObject({ kind: "text", style: "caption" });
  });

  it("changes parameter placement targets without a label override", () => {
    renderHarness(designBlockIdForParameter("month"));

    // The owned parameter label heads the panel; no label input exists.
    expect(
      screen.getByRole("heading", { name: "Month" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("textbox")).toBeNull();

    fireEvent.change(screen.getByLabelText("Parameters"), {
      target: { value: "region" },
    });

    expect(harnessCalls.draft).toHaveLength(1);
    const next = harnessCalls.draft[0];
    const items = toStudioDocument(next).spec.layout.rows.flatMap((row) =>
      row.items
    );
    expect(items).toContainEqual({ kind: "parameter", parameter: "region" });
    expect(items).not.toContainEqual({
      kind: "parameter",
      parameter: "month",
    });
    // Both declarations stay owned by the Parameters section.
    expect(next.parameters.map((parameter) => parameter.id).sort()).toEqual([
      "month",
      "region",
    ]);
  });

  it("exposes a data jump for the selected block source", () => {
    renderHarness("disp-1");

    fireEvent.click(screen.getByRole("button", { name: "Data" }));

    expect(harnessCalls.jump).toEqual([{ sourceDraftId: "src-1" }]);
    expect(harnessCalls.draft).toHaveLength(0);
  });

  it("renders nothing without a resolvable selection", () => {
    const cleared = render(() => (
      <Harness initial={seedDraft()} selectedId={null} />
    ));
    expect(cleared.container.querySelector("aside")).toBeNull();
    expect(cleared.container.textContent).toBe("");
    cleared.unmount();

    const { container, unmount } = render(() => (
      <Harness initial={seedDraft()} selectedId="disp-999" />
    ));
    expect(container.querySelector("aside")).toBeNull();
    expect(container.textContent).toBe("");
    unmount();

    // Unplaced parameters have no canvas block either.
    render(() => (
      <Harness
        initial={seedDraft()}
        selectedId={designBlockIdForParameter("region")}
      />
    ));
    expect(screen.queryByRole("complementary")).toBeNull();
    expect(screen.queryByRole("heading")).toBeNull();
  });
});
