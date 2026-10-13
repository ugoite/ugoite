import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSignal } from "solid-js";
import {
  CompositionInspector,
  type CompositionInspectorDataJump,
} from "./CompositionInspector";
import {
  addEntryQuerySource,
  addInputComponent,
  addMetricDisplay,
  addSavedSqlSource,
  addTableDisplay,
  addTextDisplay,
  addVariable,
  type CompositionDraft,
  createEmptyDraft,
  setInputLabel,
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
  const month = addVariable(draft, {
    id: "month",
    label: "Month",
    type: "date",
    required: true,
  });
  if (!month.ok) throw new Error("expected month variable");
  draft = month.draft;
  const region = addVariable(draft, {
    id: "region",
    label: "Region",
    type: "string",
    required: false,
  });
  if (!region.ok) throw new Error("expected region variable");
  draft = region.draft;
  const metric = addMetricDisplay(
    draft,
    "src-1",
    "total",
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
  const placed = addInputComponent(draft, "month");
  if (!placed.ok) throw new Error("expected variable placement");
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
  },
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
      onRemove={() => harnessCalls.remove.push(props.selectedId)}
      onDataJump={(jump) => harnessCalls.jump.push(jump)}
    />
  );
}

const renderHarness = (
  selectedId: string | null,
  initial?: CompositionDraft,
) =>
  render(() => (
    <Harness
      initial={initial ?? seedDraft()}
      selectedId={selectedId}
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

  it("removes the selected Input component without removing its variable", () => {
    renderHarness("input-4");

    const remove = screen.getByRole("button", {
      name: "Remove Input for Month",
    });
    expect(remove).toHaveAttribute("title", "Remove Input for Month");
    fireEvent.click(remove);

    expect(harnessCalls.remove).toEqual(["input-4"]);
  });

  it("targets the selected Input component when a variable has siblings", () => {
    const initial = seedDraft();
    const second = addInputComponent(initial, "month");
    if (!second.ok) throw new Error("expected sibling Input");
    const labeled = setInputLabel(
      second.draft,
      second.draftId!,
      "Secondary month",
    );
    if (!labeled.ok) throw new Error("expected Input label");
    renderHarness(second.draftId!, labeled.draft);

    fireEvent.click(
      screen.getByRole("button", { name: "Remove Input for Secondary month" }),
    );

    expect(harnessCalls.remove).toEqual([second.draftId]);
  });

  it("limits Metric source and column controls to Saved SQL names", () => {
    renderHarness("disp-1");

    const source = screen.getByLabelText("Source");
    expect(
      Array.from(source.querySelectorAll("option")).map((option) =>
        option.textContent
      ),
    ).toEqual(["Monthly", "Backup"]);
    expect(screen.queryByLabelText("Operation")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Field")).not.toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("src-1");
    expect(document.body).not.toHaveTextContent("sql-1");
    expect(document.body).not.toHaveTextContent("sql-rev-1");

    fireEvent.change(screen.getByLabelText("Source"), {
      target: { value: "src-3" },
    });
    expect(metricComponent(harnessCalls.draft.at(-1)!, "disp-1"))
      .toMatchObject({ kind: "metric", source: "src-3" });
    expect(metricComponent(harnessCalls.draft.at(-1)!, "disp-1"))
      .toHaveProperty("column", "");
    fireEvent.change(screen.getByLabelText("Column"), {
      target: { value: "count" },
    });
    const last = harnessCalls.draft[harnessCalls.draft.length - 1];
    expect(metricComponent(last, "disp-1")).toMatchObject({
      kind: "metric",
      source: "src-3",
      column: "count",
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
      source: "src-1",
      column: "count",
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
      source: "src-1",
      column: "total",
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
    const dateSource = addSavedSqlSource(createEmptyDraft("Studio"), {
      entryId: "sql-date",
      revisionId: "sql-date-rev",
      name: "Calendar",
      expectedResult: [{ name: "last_day", type: "date" }],
      variables: {},
    });
    const added = addMetricDisplay(
      dateSource.draft,
      dateSource.draftId,
      "last_day",
    );
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
      content: { kind: "literal", text: "New heading" },
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

  it("preserves variable-bound Text while editing its style", () => {
    const initial = seedDraft();
    const variableText: CompositionDraft = {
      ...initial,
      displays: initial.displays.map((display) =>
        display.kind === "text"
          ? {
            ...display,
            content: { kind: "variable" as const, variable: "region" },
          }
          : display
      ),
    };
    renderHarness("disp-3", variableText);

    expect(screen.getByLabelText("Content")).toHaveValue("variable:region");
    expect(screen.queryByRole("textbox", { name: "Text" })).toBeNull();
    fireEvent.change(screen.getByLabelText("Style"), {
      target: { value: "caption" },
    });

    expect(harnessCalls.draft).toHaveLength(1);
    expect(
      harnessCalls.draft[0].displays.find((display) =>
        display.draftId === "disp-3"
      ),
    ).toMatchObject({
      kind: "text",
      content: { kind: "variable", variable: "region" },
      style: "caption",
    });
  });

  it("changes the selected Input's variable binding", () => {
    renderHarness("input-4");

    // The owned variable label heads the panel; the optional Input label is
    // blank until the author supplies an override.
    expect(
      screen.getByRole("heading", { name: "Month" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Input label for Month")).toHaveValue("");

    fireEvent.change(screen.getByLabelText("Variables"), {
      target: { value: "region" },
    });

    expect(harnessCalls.draft).toHaveLength(1);
    const next = harnessCalls.draft[0];
    const items = toStudioDocument(next).spec.layout.rows.flatMap((row) =>
      row.items
    );
    expect(items).toContainEqual({
      kind: "component",
      component: "input-4",
    });
    expect(items).not.toContainEqual({
      kind: "component",
      component: "input-region",
    });
    expect(toStudioDocument(next).spec.components).toContainEqual({
      kind: "input",
      id: "input-4",
      variable: "region",
    });
    // Both declarations stay owned by the Variables section.
    expect(next.variables.map((variable) => variable.id).sort()).toEqual([
      "month",
      "region",
    ]);
  });

  it("uses localized ordinal labels for unlabeled variable placement", () => {
    const labeled = seedDraft();
    const draft = {
      ...labeled,
      variables: labeled.variables.map(({ label: _label, ...variable }) =>
        variable
      ),
    };
    renderHarness("input-4", draft);

    expect(screen.getByRole("heading", { name: "Variable 1" }))
      .toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Remove Input for Variable 1" }),
    )
      .toBeInTheDocument();
    const options = screen.getAllByRole("option").map((option) =>
      option.textContent
    );
    expect(options).toEqual(["Variable 1", "Variable 2"]);
    expect(screen.queryByText("month")).not.toBeInTheDocument();
    expect(screen.queryByText("region")).not.toBeInTheDocument();

    setLocale("ja");
    expect(screen.getByRole("heading", { name: "変数1" }))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "変数1の入力を削除" }))
      .toBeInTheDocument();
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

    // Unplaced variables have no canvas block either.
    render(() => (
      <Harness
        initial={seedDraft()}
        selectedId="input-region"
      />
    ));
    expect(screen.queryByRole("complementary")).toBeNull();
    expect(screen.queryByRole("heading")).toBeNull();
  });
});
