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
    { field_id: 3, field_type: "date" },
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
    { kind: "sql_scalar", column: "total" },
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
  remove: [] as (string | null)[],
}));

function Harness(
  props: {
    initial: CompositionDraft;
    selectedId: string | null;
    fieldNames?: (formId: string, fieldId: number) => string | undefined;
    fieldNamesLoading?: boolean;
  },
) {
  const [draft, setDraft] = createSignal(props.initial);
  return (
    <CompositionInspector
      draft={draft()}
      selectedId={props.selectedId}
      fieldNames={props.fieldNames}
      fieldNamesLoading={props.fieldNamesLoading}
      onDraftChange={(next) => {
        harnessCalls.draft.push(next);
        setDraft(next);
      }}
      onRemove={() => harnessCalls.remove.push(props.selectedId)}
      onDataJump={(jump) => harnessCalls.jump.push(jump)}
    />
  );
}

const renderHarness = (
  selectedId: string | null,
  initial?: CompositionDraft,
  fieldNames?: (formId: string, fieldId: number) => string | undefined,
) =>
  render(() => (
    <Harness
      initial={initial ?? seedDraft()}
      selectedId={selectedId}
      fieldNames={fieldNames}
    />
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
    harnessCalls.remove.length = 0;
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

  it("exposes removal from the selected block Inspector with a human name", () => {
    renderHarness("disp-3");

    const remove = screen.getByRole("button", { name: "Remove Summary" });
    expect(remove).toHaveAttribute("title", "Remove Summary");
    fireEvent.click(remove);

    expect(harnessCalls.remove).toEqual(["disp-3"]);
  });

  it("labels parameter removal as unplacing its canvas control", () => {
    renderHarness(designBlockIdForParameter("month"));

    const unplace = screen.getByRole("button", { name: "Unplace Month" });
    expect(unplace).toHaveAttribute("title", "Unplace Month");
    fireEvent.click(unplace);

    expect(harnessCalls.remove).toEqual(["param:month"]);
  });

  it("shows only resolved Form labels for metric values", async () => {
    const added = addMetricDisplay(seedDraft(), "src-2", {
      kind: "aggregate",
      operation: "count_values",
      fieldId: 1,
    });
    if (!added.ok || !added.draftId) throw new Error("expected metric block");

    render(() => (
      <CompositionInspector
        draft={added.draft}
        selectedId={added.draftId}
        fieldNames={(_formId, fieldId) =>
          fieldId === 1 ? "Expense type" : undefined}
        onDraftChange={() => {}}
        onRemove={() => {}}
        onDataJump={() => {}}
      />
    ));

    await screen.findByRole("option", { name: "Expense type" });
    const fieldSelect = screen.getByLabelText("Field");
    const options = Array.from(fieldSelect.querySelectorAll("option")).map(
      (option) => option.textContent,
    );
    expect(options).toEqual(["Choose a field", "Expense type"]);
    expect(options.join(" ")).not.toContain("1");
    expect(options.join(" ")).not.toContain("2");
  });

  it("maps metric source, operation, field, and SQL column edits", async () => {
    renderHarness(
      "disp-1",
      undefined,
      (_formId, fieldId) =>
        fieldId === 1 ? "Expense type" : fieldId === 2 ? "Amount" : undefined,
    );

    fireEvent.change(screen.getByLabelText("Source"), {
      target: { value: "src-2" },
    });
    expect(harnessCalls.draft).toHaveLength(1);
    expect(metricComponent(harnessCalls.draft[0], "disp-1")).toMatchObject({
      kind: "metric",
      definition: {
        kind: "aggregate",
        source: "src-2",
        operation: "count_rows",
      },
    });
    fireEvent.change(screen.getByLabelText("Operation"), {
      target: { value: "sum" },
    });
    await screen.findByRole("option", { name: "Amount" });
    fireEvent.change(screen.getByLabelText("Field"), {
      target: { value: "2" },
    });
    expect(metricComponent(harnessCalls.draft.at(-1)!, "disp-1"))
      .toMatchObject({
        definition: {
          kind: "aggregate",
          source: "src-2",
          operation: "sum",
          field_id: 2,
        },
      });
    expect(
      Array.from(screen.getByLabelText("Field").querySelectorAll("option"))
        .map((option) => option.textContent),
    ).toEqual(["Choose a field", "Amount"]);

    // A source change starts with the SQL scalar operation, and the column
    // label remains the result name rather than an internal source id.
    fireEvent.change(screen.getByLabelText("Source"), {
      target: { value: "src-3" },
    });
    fireEvent.change(screen.getByLabelText("Column"), {
      target: { value: "total" },
    });
    const last = harnessCalls.draft[harnessCalls.draft.length - 1];
    expect(metricComponent(last, "disp-1")).toMatchObject({
      kind: "metric",
      definition: {
        kind: "sql_scalar",
        source: "src-3",
        column: "total",
      },
    });
  });

  it("maps Saved SQL column changes to a typed scalar definition", () => {
    renderHarness("disp-1");

    fireEvent.change(screen.getByLabelText("Column"), {
      target: { value: "count" },
    });

    expect(harnessCalls.draft).toHaveLength(1);
    const next = harnessCalls.draft[0];
    expect(metricComponent(next, "disp-1")).toMatchObject({
      kind: "metric",
      definition: {
        kind: "sql_scalar",
        source: "src-1",
        column: "count",
      },
    });
  });

  it("edits optional numeric presentation without changing source bindings", () => {
    const initial = seedDraft();
    const before = documentSources(initial);
    renderHarness("disp-1", initial);

    const unitInput = screen.getByLabelText("Unit");
    fireEvent.input(unitInput, { target: { value: "US " } });
    expect(unitInput).toHaveValue("US ");
    fireEvent.input(unitInput, { target: { value: "US dollars" } });
    fireEvent.blur(unitInput);
    fireEvent.input(screen.getByLabelText("Minimum decimal places"), {
      target: { value: "2" },
    });

    const formatted = harnessCalls.draft.at(-1)!;
    expect(metricComponent(formatted, "disp-1")).toMatchObject({
      kind: "metric",
      definition: {
        kind: "sql_scalar",
        source: "src-1",
        column: "total",
      },
      presentation: {
        kind: "number",
        unit: "US dollars",
        fraction_digits: 2,
      },
    });
    expect(documentSources(formatted)).toBe(before);

    fireEvent.input(screen.getByLabelText("Unit"), {
      target: { value: "" },
    });
    fireEvent.input(screen.getByLabelText("Minimum decimal places"), {
      target: { value: "" },
    });
    expect(metricComponent(harnessCalls.draft.at(-1)!, "disp-1"))
      .not.toHaveProperty("presentation");
  });

  it("hides numeric presentation controls for date Metrics", () => {
    const added = addMetricDisplay(seedDraft(), "src-2", {
      kind: "aggregate",
      operation: "max",
      fieldId: 3,
    });
    if (!added.ok || !added.draftId) throw new Error("expected Metric block");

    renderHarness(added.draftId, added.draft);

    expect(screen.queryByLabelText("Unit")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Minimum decimal places"))
      .not.toBeInTheDocument();
  });

  it("localizes numeric presentation control names", () => {
    setLocale("ja");
    renderHarness("disp-1");

    expect(screen.getByLabelText("単位")).toBeInTheDocument();
    expect(screen.getByLabelText("小数部の最低桁数")).toBeInTheDocument();
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
