import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSignal } from "solid-js";
import {
  CompositionDesignCanvas,
  designBlockIdForComponent,
  designBlockIdForParameter,
} from "./CompositionDesignCanvas";
import {
  addParameter,
  addSavedSqlSource,
  addTableDisplay,
  addTextDisplay,
  type CompositionDraft,
  createEmptyDraft,
  type DraftInsertTarget,
  moveDisplay,
  placeParameterControl,
  toStudioDocument,
} from "~/lib/composition-draft";
import { setLocale } from "~/lib/i18n";

const stylesheet = () => readFileSync(join(__dirname, "..", "app.css"), "utf8");

const sqlSeed = () => ({
  entryId: "sql-1",
  revisionId: "rev-1",
  name: "Monthly",
  expectedResult: [{ name: "total", type: "float" as const }],
  variables: {},
});

/** One heading text, one table, one placed parameter across two rows. */
const seedDraft = (): CompositionDraft => {
  let draft = createEmptyDraft("Studio");
  draft = addSavedSqlSource(draft, sqlSeed()).draft;
  const month = addParameter(draft, {
    id: "month",
    label: "Month",
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
  if (!placed.ok) throw new Error("expected parameter placement");
  return placed.draft;
};

const harnessCalls = vi.hoisted(() => ({
  draft: [] as CompositionDraft[],
  selected: [] as (string | null)[],
  picker: [] as DraftInsertTarget[],
  parameters: [] as [string, unknown | undefined][],
}));

function Harness(
  props: { initial: CompositionDraft; highlightedIds?: ReadonlySet<string> },
) {
  const [draft, setDraft] = createSignal(props.initial);
  const [selectedId, setSelectedId] = createSignal<string | null>(null);
  const [paletteTarget, setPaletteTarget] = createSignal<
    DraftInsertTarget | null
  >(
    null,
  );
  return (
    <CompositionDesignCanvas
      draft={draft()}
      plan={{
        sources: [],
        component_bindings: draft().displays
          .filter((display) => display.kind === "text")
          .map((display) => ({
            component_id: display.draftId,
            kind: "text" as const,
          })),
      }}
      parameterValues={{}}
      sources={{}}
      selectedId={selectedId()}
      highlightedIds={props.highlightedIds}
      onSelect={(id) => {
        harnessCalls.selected.push(id);
        setSelectedId(id);
      }}
      onDraftChange={(next) => {
        harnessCalls.draft.push(next);
        setDraft(next);
      }}
      onRequestDisplayPicker={(target) => harnessCalls.picker.push(target)}
      onParameterChange={(parameterId, value) => {
        harnessCalls.parameters.push([parameterId, value]);
      }}
      onNext={() => {}}
      onPrevious={() => {}}
      onRetry={() => {}}
      paletteTarget={paletteTarget()}
      onPaletteTarget={setPaletteTarget}
    />
  );
}

const renderHarness = (
  initial?: CompositionDraft,
  highlightedIds?: ReadonlySet<string>,
) =>
  render(() => (
    <Harness initial={initial ?? seedDraft()} highlightedIds={highlightedIds} />
  ));

describe("CompositionDesignCanvas", () => {
  beforeEach(() => {
    setLocale("en");
    vi.clearAllMocks();
    harnessCalls.draft.length = 0;
    harnessCalls.selected.length = 0;
    harnessCalls.picker.length = 0;
    harnessCalls.parameters.length = 0;
  });

  afterEach(() => cleanup());

  it("selects canvas blocks without touching the draft", () => {
    const { container } = renderHarness();

    // Two placed rows plus the parameter block: three selectable frames.
    const frames = container.querySelectorAll(".designBlock");
    expect(frames).toHaveLength(3);

    fireEvent.click(frames[0]);
    expect(harnessCalls.selected).toEqual([
      designBlockIdForParameter("month"),
    ]);
    expect(harnessCalls.draft).toHaveLength(0);

    // The select toggle exposes pressed state with a visible focus path.
    const toggle = screen.getByRole("button", { name: "Select Month" });
    expect(toggle).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(toggle);
    expect(harnessCalls.selected).toEqual([
      designBlockIdForParameter("month"),
      null,
    ]);
    expect(harnessCalls.draft).toHaveLength(0);

    // The selected block carries the edit outline; others do not.
    fireEvent.click(screen.getByRole("button", { name: "Select Summary" }));
    const selected = container.querySelectorAll(".designBlock[data-selected]");
    expect(selected).toHaveLength(1);
    expect(selected[0]).toHaveAttribute(
      "data-block-id",
      designBlockIdForComponent("disp-1"),
    );
  });

  it("soft-highlights source blocks without changing the selection", () => {
    const { container } = renderHarness(
      undefined,
      new Set([designBlockIdForComponent("disp-2")]),
    );

    const table = container.querySelector('[data-block-id="disp-2"]');
    expect(table).toHaveAttribute("data-highlighted");
    expect(table).not.toHaveAttribute("data-selected");
    // Highlight never selects: the canvas keeps exactly one selection owner.
    expect(
      container.querySelectorAll(".designBlock[data-selected]"),
    ).toHaveLength(0);
  });

  it("shows one palette entry per component kind with icon and short label", () => {
    renderHarness();

    fireEvent.click(screen.getAllByRole("button", { name: "Add block" })[0]);
    const dialog = screen.getByRole("dialog", { name: "Add block" });

    // Text, metric, and table entries each pair an icon with a short label.
    expect(dialog.querySelectorAll(".designPaletteItem svg")).not.toHaveLength(
      0,
    );
    const entries = Array.from(
      dialog.querySelectorAll(".designPaletteItem"),
    ).map((entry) => entry.textContent);
    expect(entries.join(" ")).toContain("Text");
    expect(entries.join(" ")).toContain("Metric");
    expect(entries.join(" ")).toContain("Table");

    // The placed month control leaves no parameter to offer.
    expect(dialog.querySelector(".designPaletteParams")).toHaveTextContent(
      "Add a parameter to begin.",
    );
  });

  it("inserts text blocks declared and placed exactly once", () => {
    renderHarness();

    fireEvent.click(screen.getAllByRole("button", { name: "Add block" })[0]);
    fireEvent.click(
      screen.getByRole("dialog", { name: "Add block" }).querySelector(
        ".designPaletteEntries",
      )!.querySelectorAll("button")[0],
    );

    expect(harnessCalls.draft).toHaveLength(1);
    expect(harnessCalls.picker).toHaveLength(0);
    const document = toStudioDocument(harnessCalls.draft[0]);
    const texts = document.spec.components.filter(
      (component) => component.kind === "text",
    );
    expect(texts).toHaveLength(2);
    const placed = document.spec.layout.rows.flatMap((row) => row.items).filter(
      (item) => item.kind === "component" && item.component === "disp-3",
    );
    expect(placed).toHaveLength(1);
    // Selection follows the inserted block for the inspector.
    expect(harnessCalls.selected).toEqual([
      designBlockIdForComponent("disp-3"),
    ]);
  });

  it("places existing parameter controls without creating parameters", () => {
    let draft = createEmptyDraft("Studio");
    draft = addSavedSqlSource(draft, sqlSeed()).draft;
    const region = addParameter(draft, {
      id: "region",
      type: "string",
      required: false,
    });
    if (!region.ok) throw new Error("expected parameter");
    draft = region.draft;
    renderHarness(draft);

    fireEvent.click(screen.getAllByRole("button", { name: "Add block" })[0]);
    const dialog = screen.getByRole("dialog", { name: "Add block" });
    fireEvent.click(
      Array.from(dialog.querySelectorAll(".designPaletteItem")).find((entry) =>
        entry.textContent === "region"
      ) as HTMLElement,
    );

    expect(harnessCalls.draft).toHaveLength(1);
    const next = harnessCalls.draft[0];
    // The semantic parameter list is untouched; only placement changes.
    expect(next.parameters.map((parameter) => parameter.id)).toEqual([
      "region",
    ]);
    const document = toStudioDocument(next);
    expect(
      document.spec.layout.rows.flatMap((row) => row.items),
    ).toContainEqual({ kind: "parameter", parameter: "region" });
    expect(harnessCalls.selected).toEqual([
      designBlockIdForParameter("region"),
    ]);
  });

  it("delegates metric and table insertion to the existing display picker", () => {
    renderHarness();

    fireEvent.click(screen.getAllByRole("button", { name: "Add block" })[0]);
    const dialog = screen.getByRole("dialog", { name: "Add block" });
    const entries = dialog.querySelector(".designPaletteEntries")!
      .querySelectorAll("button");
    fireEvent.click(entries[1]);

    // No draft change: the shared picker owns source and value selection.
    expect(harnessCalls.draft).toHaveLength(0);
    expect(harnessCalls.picker).toHaveLength(1);
    expect(harnessCalls.picker[0].rowId).toBeNull();
    expect(screen.queryByRole("dialog", { name: "Add block" })).toBeNull();
  });

  it("reorders rows and items with keyboard-operable buttons", () => {
    const { container } = renderHarness();

    // Row bars reuse the list up/down pattern with row ordinals.
    const rowUp = screen.getByRole("button", { name: "Move row 2 up" });
    expect(rowUp).toHaveAttribute("title", "Move row 2 up");
    fireEvent.click(rowUp);
    expect(harnessCalls.draft).toHaveLength(1);
    let rows = toStudioDocument(harnessCalls.draft[0]).spec.layout.rows;
    expect(rows[0].items).toEqual([
      { kind: "component", component: "disp-2" },
    ]);

    // Item moves appear on the selected block only, keeping one owner.
    expect(
      screen.queryByRole("button", { name: "Move Month up" }),
    ).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Select Month" }));
    const itemDown = screen.getByRole("button", { name: "Move Month down" });
    fireEvent.click(itemDown);
    expect(harnessCalls.draft).toHaveLength(2);
    rows = toStudioDocument(harnessCalls.draft[1]).spec.layout.rows;
    const headingRow = rows.find((row) =>
      row.items.some((item) =>
        item.kind === "component" && item.component === "disp-1"
      )
    );
    expect(headingRow?.items).toEqual([
      { kind: "component", component: "disp-1" },
      { kind: "parameter", parameter: "month" },
    ]);

    // Every reorder control is a native button in visual order.
    const names = Array.from(
      container.querySelectorAll(
        ".designRowBar button, .designBlockActions button",
      ),
    ).map((button) => button.getAttribute("aria-label"));
    expect(names).toContain("Move row 1 down");
    expect(names).toContain("Move Month up");
    expect(container.querySelectorAll("button[aria-label]")).not.toHaveLength(
      0,
    );
  });

  it("removes component declarations and unplaces parameter controls", () => {
    renderHarness();

    fireEvent.click(screen.getByRole("button", { name: "Select Details" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove Details" }));
    expect(harnessCalls.draft).toHaveLength(1);
    const afterTable = harnessCalls.draft[0];
    expect(afterTable.displays.map((display) => display.draftId)).toEqual([
      "disp-1",
    ]);
    expect(
      toStudioDocument(afterTable).spec.layout.rows.flatMap((row) => row.items),
    ).not.toContainEqual({ kind: "component", component: "disp-2" });
    expect(harnessCalls.selected.slice(-1)).toEqual([null]);

    fireEvent.click(screen.getByRole("button", { name: "Select Month" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove Month" }));
    expect(harnessCalls.draft).toHaveLength(2);
    const afterParam = harnessCalls.draft[1];
    // Unplacing keeps the semantic parameter declared for the Data section.
    expect(afterParam.parameters.map((parameter) => parameter.id)).toEqual([
      "month",
    ]);
    expect(
      toStudioDocument(afterParam).spec.layout.rows.flatMap((row) => row.items),
    ).not.toContainEqual({ kind: "parameter", parameter: "month" });
  });

  it("routes block operations through the draft channel without extra queries", () => {
    renderHarness();

    // Text insert, row move, and selection touch only the draft channel.
    fireEvent.click(screen.getAllByRole("button", { name: "Add block" })[0]);
    fireEvent.click(
      screen.getByRole("dialog", { name: "Add block" }).querySelector(
        ".designPaletteEntries",
      )!.querySelectorAll("button")[0],
    );
    fireEvent.click(screen.getByRole("button", { name: "Move row 2 up" }));
    fireEvent.click(screen.getByRole("button", { name: "Select Summary" }));

    expect(harnessCalls.picker).toHaveLength(0);
    expect(harnessCalls.parameters).toHaveLength(0);
    expect(harnessCalls.draft.length).toBeGreaterThan(0);
    // Sources never change shape across text and placement operations.
    const sources = harnessCalls.draft.map((draft) =>
      JSON.stringify(toStudioDocument(draft).spec.sources)
    );
    expect(new Set(sources).size).toBe(1);
  });

  it("keeps canvas blocks on shared flow classes for narrow viewports", () => {
    const { container } = renderHarness();

    expect(
      container.querySelector(".designCanvas.compositionFlow"),
    ).not.toBeNull();
    expect(
      container.querySelector(".designCanvas .compositionFlowRow"),
    ).not.toBeNull();
    expect(
      container.querySelector(".designCanvas .compositionFlowItem--parameter"),
    ).not.toBeNull();
    expect(
      container.querySelector(".designCanvas .compositionFlowItem--text"),
    ).not.toBeNull();
    const css = stylesheet();
    // Narrow viewports stack metric and parameter blocks while tables keep
    // scrolling in their own viewport; inline gaps go full-width instead of
    // squeezing the 390px single column.
    expect(css).toMatch(
      /@media\s*\(max-width:\s*560px\)[\s\S]*?\.compositionFlowItem--metric\s*\{[\s\S]*?flex-basis:\s*100%/,
    );
    expect(css).toMatch(
      /\.designGap--inline\s*\{[^}]*min-width:\s*44px/,
    );
    expect(css).toMatch(
      /@media\s*\(max-width:\s*560px\)[\s\S]*?\.designGap--inline\s*\{[\s\S]*?flex-basis:\s*100%/,
    );
  });

  it("keeps legacy display moves in sync with the two-dimensional layout", () => {
    const moved = moveDisplay(seedDraft(), "disp-2", "up");
    expect(moved.ok).toBe(true);
    if (!moved.ok) throw new Error("expected display move");
    const document = toStudioDocument(moved.draft);
    expect(document.spec.components.map((component) => component.id)).toEqual([
      "disp-2",
      "disp-1",
    ]);
    // Cross-row neighbors exchange positions so every component stays placed.
    expect(
      document.spec.layout.rows.flatMap((row) => row.items),
    ).toContainEqual({ kind: "component", component: "disp-2" });
    expect(
      document.spec.layout.rows.flatMap((row) => row.items),
    ).toContainEqual({ kind: "component", component: "disp-1" });
    expect(
      document.spec.layout.rows.flatMap((row) => row.items),
    ).toContainEqual({ kind: "parameter", parameter: "month" });
  });
});
