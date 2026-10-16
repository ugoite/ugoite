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
  type CompositionDraft,
  createEmptyDraft,
} from "~/lib/composition-draft";
import { setLocale } from "~/lib/i18n";

const harnessCalls = {
  draft: [] as CompositionDraft[],
  jump: [] as CompositionInspectorDataJump[],
  remove: 0,
};

const seedDraft = (): CompositionDraft => {
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
  const metric = addMetricDisplay(draft, "src-1", "total");
  if (!metric.ok) throw new Error("expected metric block");
  draft = metric.draft;
  const table = addTableDisplay(draft, "src-2");
  if (!table.ok) throw new Error("expected table block");
  draft = table.draft;
  const text = addTextDisplay(draft, { text: "Summary", style: "heading" });
  if (!text.ok) throw new Error("expected text block");
  draft = text.draft;
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
    />
  );
}

const renderInspector = (selectedId: string) =>
  render(() => <Harness initial={seedDraft()} selectedId={selectedId} />);

describe("CompositionInspector SQL-first fields", () => {
  beforeEach(() => {
    setLocale("en");
    harnessCalls.draft = [];
    harnessCalls.jump = [];
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
    expect(screen.getByLabelText("Source")).toHaveValue("src-2");
    expect(screen.queryByLabelText("Label")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Data" }));
    expect(harnessCalls.jump).toEqual([{ sourceDraftId: "src-2" }]);
  });

  it("uses Text content and style as its only editable presentation", () => {
    renderInspector("disp-3");
    expect(screen.getByLabelText("Content")).toHaveValue("literal");
    expect(screen.getByLabelText("Text", { selector: "input" }))
      .toHaveValue("Summary");
    expect(screen.getByLabelText("Style")).toHaveValue("heading");
    expect(screen.queryByLabelText("Label")).not.toBeInTheDocument();
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
