import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  CompositionInspector,
  type CompositionInspectorDataJump,
} from "./CompositionInspector";
import {
  addInlineSqlSource,
  addInputComponent,
  addMetricDisplay,
  addSavedSqlSource,
  addTableDisplay,
  addTextDisplay,
  addVariable,
  setTextContent,
  type CompositionDraft,
  createEmptyDraft,
} from "~/lib/composition-draft";
import { setLocale } from "~/lib/i18n";

const harnessCalls = {
  draft: [] as CompositionDraft[],
  jump: [] as CompositionInspectorDataJump[],
  changeSource: [] as string[],
  remove: 0,
};

const seedDraft = (
  withStringVariable = false,
  withSecondText = false,
): CompositionDraft => {
  let draft = createEmptyDraft("Studio");
  draft = addSavedSqlSource(draft, {
    entryId: "sql-1",
    revisionId: "rev-1",
    name: "Monthly totals",
    expectedResult: [
      { name: "total", type: "float" },
      { name: "count", type: "integer" },
    ],
    variables: {},
  }).draft;
  draft = addInlineSqlSource(draft, {
    name: "Expenses",
    sql: 'SELECT "amount" FROM "Expenses"',
    formBindings: [],
    expectedResult: [{ name: "amount", type: "float" }],
  }).draft;
  const month = addVariable(draft, {
    id: "month",
    type: "date",
    required: false,
  });
  if (!month.ok) throw new Error("expected month variable");
  draft = month.draft;
  if (withStringVariable) {
    const title = addVariable(draft, {
      id: "report_title",
      type: "string",
      required: false,
    });
    if (!title.ok) throw new Error("expected string variable");
    draft = title.draft;
  }
  const metric = addMetricDisplay(draft, "src-1", "total");
  if (!metric.ok) throw new Error("expected metric block");
  draft = metric.draft;
  const table = addTableDisplay(draft, "src-2");
  if (!table.ok) throw new Error("expected table block");
  draft = table.draft;
  const text = addTextDisplay(draft, { text: "Summary", style: "heading" });
  if (!text.ok) throw new Error("expected text block");
  draft = text.draft;
  if (withSecondText) {
    const secondText = addTextDisplay(draft, {
      text: "Revenue",
      style: "body",
    });
    if (!secondText.ok) throw new Error("expected second text block");
    draft = secondText.draft;
  }
  const input = addInputComponent(draft, "month");
  if (!input.ok) throw new Error("expected variable placement");
  return input.draft;
};

function Harness(props: { initial: CompositionDraft; selectedId: string }) {
  const [draft, setDraft] = createSignal(props.initial);
  return (
    <CompositionInspector
      draft={draft()}
      selectedId={props.selectedId}
      onDraftChange={(next) => {
        harnessCalls.draft.push(next);
        setDraft(next);
      }}
      onRemove={() => harnessCalls.remove++}
      onDataJump={(jump) => harnessCalls.jump.push(jump)}
      onChangeTableSource={(displayId) =>
        harnessCalls.changeSource.push(displayId)}
    />
  );
}

function SelectableTextHarness(props: { initial: CompositionDraft }) {
  const [draft, setDraft] = createSignal(props.initial);
  const [selectedId, setSelectedId] = createSignal("disp-3");
  return (
    <>
      <button type="button" onClick={() => setSelectedId("disp-4")}>
        Select second Text
      </button>
      <button
        type="button"
        onClick={() => setSelectedId(props.initial.inputs[0].draftId)}
      >
        Select Input
      </button>
      <button type="button" onClick={() => setSelectedId("disp-3")}>
        Select first Text
      </button>
      <button
        type="button"
        onClick={() => {
          const result = setTextContent(draft(), "disp-3", "Updated summary");
          if (result.ok) setDraft(result.draft);
        }}
      >
        Update text externally
      </button>
      <CompositionInspector
        draft={draft()}
        selectedId={selectedId()}
        onDraftChange={(next) => {
          harnessCalls.draft.push(next);
          setDraft(next);
        }}
        onRemove={() => harnessCalls.remove++}
        onDataJump={(jump) => harnessCalls.jump.push(jump)}
        onChangeTableSource={(displayId) =>
          harnessCalls.changeSource.push(displayId)}
      />
    </>
  );
}

const renderInspector = (selectedId: string) =>
  render(() => <Harness initial={seedDraft()} selectedId={selectedId} />);

describe("CompositionInspector SQL-first fields", () => {
  beforeEach(() => {
    setLocale("en");
    harnessCalls.draft = [];
    harnessCalls.jump = [];
    harnessCalls.changeSource = [];
    harnessCalls.remove = 0;
  });

  afterEach(() => cleanup());

  it("edits a Metric source column without a persisted display label", () => {
    renderInspector("disp-1");
    expect(screen.getByRole("heading", { name: "Metric" }))
      .toBeInTheDocument();
    expect(screen.getByLabelText("Column")).toHaveValue("total");
    expect(screen.queryByLabelText("Label")).not.toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Column"), {
      target: { value: "count" },
    });
    expect(harnessCalls.draft.at(-1)?.displays[0]).toMatchObject({
      kind: "metric",
      column: "count",
    });
  });

  it("keeps Table source identity and data navigation without a label editor", () => {
    renderInspector("disp-2");
    expect(screen.queryByLabelText("Source")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Label")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Change source" }));
    expect(harnessCalls.changeSource).toEqual(["disp-2"]);
    fireEvent.click(screen.getByRole("button", { name: "Data" }));
    expect(harnessCalls.jump).toEqual([{ sourceDraftId: "src-2" }]);
  });

  it("shows a multiline Text body and Style without a content selector when no string variables exist", () => {
    renderInspector("disp-3");
    expect(screen.queryByLabelText("Content")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Text body", { selector: "textarea" }))
      .toHaveValue("Summary");
    expect(screen.getByLabelText("Style")).toHaveValue("heading");
    expect(screen.queryByRole("button", { name: "Use variable" }))
      .not.toBeInTheDocument();
    expect(screen.queryByLabelText("Label")).not.toBeInTheDocument();

    fireEvent.input(screen.getByLabelText("Text body"), {
      target: { value: "Quarterly summary" },
    });
    expect(harnessCalls.draft.at(-1)?.displays[2]).toMatchObject({
      kind: "text",
      content: { kind: "literal", text: "Quarterly summary" },
    });
  });

  it("binds Text to a string variable only after explicit choice and returns to literal editing", () => {
    render(() => <Harness initial={seedDraft(true)} selectedId="disp-3" />);
    expect(screen.queryByLabelText("Content")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Text body")).toHaveValue("Summary");
    expect(screen.queryByLabelText("Variable")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Use variable" }));
    expect(screen.getByLabelText("Variable")).toHaveFocus();
    fireEvent.change(screen.getByLabelText("Variable"), {
      target: { value: "report_title" },
    });

    expect(harnessCalls.draft.at(-1)?.displays[2]).toMatchObject({
      kind: "text",
      content: { kind: "variable", variable: "report_title" },
    });
    expect(screen.getAllByText("report_title")).toHaveLength(1);
    expect(screen.queryByLabelText("Text body")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Edit text" })).toHaveFocus();
    fireEvent.click(screen.getByRole("button", { name: "Edit text" }));

    expect(screen.getByLabelText("Text body")).toHaveValue("Summary");
    expect(screen.getByLabelText("Text body")).toHaveFocus();
    expect(harnessCalls.draft.at(-1)?.displays[2]).toMatchObject({
      kind: "text",
      content: { kind: "literal", text: "Summary" },
    });
  });

  it("restores the selected Text block's own literal after switching blocks", () => {
    render(() => <SelectableTextHarness initial={seedDraft(true, true)} />);
    fireEvent.click(screen.getByRole("button", { name: "Select second Text" }));
    expect(screen.getByLabelText("Text body")).toHaveValue("Revenue");

    fireEvent.click(screen.getByRole("button", { name: "Use variable" }));
    fireEvent.change(screen.getByLabelText("Variable"), {
      target: { value: "report_title" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Edit text" }));

    expect(screen.getByLabelText("Text body")).toHaveValue("Revenue");
  });

  it("restores the literal after switching to another inspector type", () => {
    render(() => <SelectableTextHarness initial={seedDraft(true)} />);
    fireEvent.click(screen.getByRole("button", { name: "Use variable" }));
    fireEvent.change(screen.getByLabelText("Variable"), {
      target: { value: "report_title" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Select Input" }));
    expect(screen.getByRole("heading", { name: "month" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Select first Text" }));
    fireEvent.click(screen.getByRole("button", { name: "Edit text" }));

    expect(screen.getByLabelText("Text body")).toHaveValue("Summary");
  });

  it("restores a literal updated outside the Text Inspector", () => {
    render(() => <SelectableTextHarness initial={seedDraft(true)} />);
    fireEvent.click(screen.getByRole("button", { name: "Update text externally" }));
    expect(screen.getByLabelText("Text body"))
      .toHaveValue("Updated summary");

    fireEvent.click(screen.getByRole("button", { name: "Use variable" }));
    fireEvent.change(screen.getByLabelText("Variable"), {
      target: { value: "report_title" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Edit text" }));

    expect(screen.getByLabelText("Text body")).toHaveValue("Updated summary");
  });

  it("presents the variable ID for an Input and does not persist an Input label", () => {
    renderInspector(seedDraft().inputs[0].draftId);
    expect(screen.getByRole("heading", { name: "month" }))
      .toBeInTheDocument();
    expect(screen.getByLabelText("Variables")).toHaveValue("month");
    expect(screen.queryByLabelText("Input label for month"))
      .not.toBeInTheDocument();
  });
});
