import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@solidjs/testing-library";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSignal } from "solid-js";
import {
  CompositionDesignCanvas,
  designBlockIdForComponent,
} from "./CompositionDesignCanvas";
import {
  addInputComponent,
  addSavedSqlSource,
  addTableDisplay,
  addTextDisplay,
  addVariable,
  type CompositionDraft,
  createEmptyDraft,
  type DraftInsertTarget,
  moveDisplay,
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

/** One heading text, one table, one placed variable across two rows. */
const seedDraft = (): CompositionDraft => {
  let draft = createEmptyDraft("Studio");
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
  if (!heading.ok) throw new Error("expected text block");
  draft = heading.draft;
  const table = addTableDisplay(draft, "src-1", {
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
  if (!placed.ok) throw new Error("expected variable placement");
  return placed.draft;
};

const harnessCalls = vi.hoisted(() => ({
  draft: [] as CompositionDraft[],
  selected: [] as (string | null)[],
  inserted: [] as string[],
  picker: [] as { target: DraftInsertTarget }[],
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
      variableValues={{}}
      sources={{}}
      selectedId={selectedId()}
      highlightedIds={props.highlightedIds}
      onSelect={(id) => {
        harnessCalls.selected.push(id);
        setSelectedId(id);
      }}
      onInsert={(id) => harnessCalls.inserted.push(id)}
      onDraftChange={(next) => {
        harnessCalls.draft.push(next);
        setDraft(next);
      }}
      onRequestDisplayPicker={(target) => harnessCalls.picker.push({ target })}
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
    harnessCalls.inserted.length = 0;
    harnessCalls.picker.length = 0;
  });

  afterEach(() => cleanup());

  it("selects canvas blocks without touching the draft", () => {
    const { container } = renderHarness();

    // Two placed rows plus the variable block: three selectable frames.
    const frames = container.querySelectorAll(".designBlock");
    expect(frames).toHaveLength(3);

    fireEvent.click(frames[0]);
    expect(harnessCalls.selected).toEqual([
      designBlockIdForComponent(seedDraft().inputs[0].draftId),
    ]);
    expect(harnessCalls.draft).toHaveLength(0);

    // The select toggle exposes pressed state with a visible focus path.
    const toggle = screen.getByRole("button", { name: "Select month" });
    expect(toggle).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(toggle);
    expect(harnessCalls.selected).toEqual([
      designBlockIdForComponent(seedDraft().inputs[0].draftId),
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

  it("selects repeated Inputs by component identity", () => {
    const initial = seedDraft();
    const added = addInputComponent(initial, "month");
    if (!added.ok) throw new Error("expected repeated Input");
    const withSecondInput = added.draft;
    renderHarness(withSecondInput);

    fireEvent.click(
      screen.getAllByRole("button", { name: "Select month" })[1],
    );

    expect(harnessCalls.selected).toEqual([
      designBlockIdForComponent(added.draftId!),
    ]);
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

  it("shows one palette entry per block kind with icon and short label", () => {
    renderHarness();

    fireEvent.click(
      screen.getAllByRole("button", { name: "Add to design" })[0],
    );
    const dialog = screen.getByRole("dialog", { name: "Add to design" });

    // Text and data entries each pair an icon with a short label.
    expect(dialog.querySelectorAll(".designPaletteItem svg")).not.toHaveLength(
      0,
    );
    const entries = Array.from(
      dialog.querySelectorAll(".designPaletteItem"),
    ).map((entry) => entry.textContent);
    expect(entries.join(" ")).toContain("Text");
    expect(entries.join(" ")).toContain("Data");
    expect(entries.join(" ")).not.toContain("Metric");
    expect(entries.join(" ")).not.toContain("Table");

    // Repeated Inputs are ordinary components, so an existing variable stays
    // available for another placement.
    expect(
      screen.getByRole("button", { name: "Add Input for month" }),
    ).toBeInTheDocument();
  });

  it("opens the data picker before a source exists", () => {
    renderHarness();

    fireEvent.click(
      screen.getAllByRole("button", { name: "Add to design" })[0],
    );
    fireEvent.click(screen.getByRole("button", { name: "Data" }));

    expect(harnessCalls.picker).toHaveLength(1);
    expect(harnessCalls.picker[0]).toEqual({
      target: { rowId: null, rowIndex: 0, itemIndex: 0 },
    });
  });

  it("opens a readable viewport-capped modal palette with focus trap and focus return", async () => {
    renderHarness();

    const gap = screen.getAllByRole("button", { name: "Add to design" })[0];
    // A real pointer tap focuses the gap control before the dialog mounts,
    // so the opener capture sees the invoking control.
    (gap as HTMLElement).focus();
    fireEvent.click(gap);
    const dialog = screen.getByRole("dialog", { name: "Add to design" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog.parentElement).toHaveClass("ui-backdrop");
    const css = stylesheet();
    expect(css).toMatch(
      /\.designPalette\s*\{[^}]*width:\s*min\(560px,\s*calc\(100vw - 32px\)\)/,
    );
    expect(css).toMatch(
      /\.designPalette\s*\{[^}]*max-height:\s*calc\(100dvh - 32px\);[^}]*overflow-y:\s*auto/,
    );

    // Focus moves into the dialog on open.
    await waitFor(() => {
      expect(dialog.querySelector("button")).toHaveFocus();
    });

    // Tab cycles within the dialog instead of reaching the canvas.
    const buttons = Array.from(dialog.querySelectorAll("button"));
    expect(buttons.length).toBeGreaterThan(1);
    buttons[buttons.length - 1].focus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(buttons[0]).toHaveFocus();

    // Escape dismisses and returns focus to the invoking gap control
    // without touching the draft.
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Add to design" })).toBeNull();
    await waitFor(() => {
      expect(gap).toHaveFocus();
    });
    expect(harnessCalls.draft).toHaveLength(0);
    expect(harnessCalls.picker).toHaveLength(0);
  });

  it("dismisses the palette on backdrop click without touching the draft", () => {
    renderHarness();

    fireEvent.click(
      screen.getAllByRole("button", { name: "Add to design" })[0],
    );
    const dialog = screen.getByRole("dialog", { name: "Add to design" });
    fireEvent.click(dialog.parentElement!);

    expect(screen.queryByRole("dialog", { name: "Add to design" })).toBeNull();
    expect(harnessCalls.draft).toHaveLength(0);
    expect(harnessCalls.picker).toHaveLength(0);
  });

  it("offers data and hides variable section without sources or variables", () => {
    renderHarness(createEmptyDraft("Studio"));

    fireEvent.click(screen.getByRole("button", { name: "Add to design" }));
    const dialog = screen.getByRole("dialog", { name: "Add to design" });
    const entries = Array.from(
      dialog.querySelectorAll(".designPaletteItem"),
    ).map((entry) => entry.textContent);

    // Text and data insertion are always available.
    expect(entries.join(" ")).toContain("Text");
    expect(entries.join(" ")).toContain("Data");
    // The variables section needs a declared variable, not an empty state.
    expect(dialog.querySelector(".designPaletteParams")).toBeNull();
  });

  it("separates text content from the block action bar with token spacing", () => {
    const css = stylesheet();
    expect(css).toMatch(
      /\.designBlockContent\s*\{\s*margin-top:\s*var\(--ui-spacing-sm\);\s*\}/,
    );
  });

  it("inserts text blocks declared and placed exactly once", () => {
    renderHarness();

    fireEvent.click(
      screen.getAllByRole("button", { name: "Add to design" })[0],
    );
    fireEvent.click(
      screen.getByRole("dialog", { name: "Add to design" }).querySelector(
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
      (item) => item.kind === "component" && item.component === "disp-4",
    );
    expect(placed).toHaveLength(1);
    expect(harnessCalls.inserted).toEqual([
      designBlockIdForComponent("disp-4"),
    ]);
    expect(harnessCalls.selected).toEqual([]);
  });

  it("shows one add control above and below a single component", () => {
    renderHarness(createEmptyDraft("Studio"));

    fireEvent.click(screen.getByRole("button", { name: "Add to design" }));
    fireEvent.click(
      screen.getByRole("dialog", { name: "Add to design" }).querySelector(
        ".designPaletteItem",
      )!,
    );

    expect(
      screen.getAllByRole("button", { name: "Add to design" }),
    ).toHaveLength(2);
  });

  it("places existing variable controls without creating variables", () => {
    let draft = createEmptyDraft("Studio");
    draft = addSavedSqlSource(draft, sqlSeed()).draft;
    const region = addVariable(draft, {
      id: "region",
      type: "string",
      required: false,
    });
    if (!region.ok) throw new Error("expected variable");
    draft = region.draft;
    renderHarness(draft);

    fireEvent.click(
      screen.getAllByRole("button", { name: "Add to design" })[0],
    );
    const dialog = screen.getByRole("dialog", { name: "Add to design" });
    fireEvent.click(
      Array.from(dialog.querySelectorAll(".designPaletteItem")).find((entry) =>
        entry.textContent === "region"
      ) as HTMLElement,
    );
    expect(screen.getByText("region")).toBeInTheDocument();

    expect(harnessCalls.draft).toHaveLength(1);
    const next = harnessCalls.draft[0];
    // The semantic variable list is untouched; only placement changes.
    expect(next.variables.map((variable) => variable.id)).toEqual([
      "region",
    ]);
    const document = toStudioDocument(next);
    expect(
      document.spec.layout.rows.flatMap((row) => row.items),
    ).toContainEqual({
      kind: "component",
      component: "input-1",
    });
    expect(harnessCalls.inserted).toEqual([
      designBlockIdForComponent("input-1"),
    ]);
    expect(harnessCalls.selected).toEqual([]);
  });

  it("uses stable variable IDs for palette entries in every locale", () => {
    let draft = createEmptyDraft("Studio");
    for (const id of ["month_key", "region_key"]) {
      const variable = addVariable(draft, {
        id,
        type: "string",
        required: false,
      });
      if (!variable.ok) throw new Error("expected variable");
      draft = variable.draft;
    }
    renderHarness(draft);

    fireEvent.click(
      screen.getAllByRole("button", { name: "Add to design" })[0],
    );
    expect(screen.getByRole("button", { name: "Add Input for month_key" }))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add Input for region_key" }))
      .toBeInTheDocument();
    expect(screen.getByText("month_key")).toBeInTheDocument();
    expect(screen.getByText("region_key")).toBeInTheDocument();

    setLocale("ja");
    expect(screen.getByRole("button", { name: "month_keyの入力を追加" }))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "region_keyの入力を追加" }))
      .toBeInTheDocument();
  });

  it("uses the stable variable ID for placed Input canvas names", () => {
    const bindingId = "private_input_key_22";
    const variable = addVariable(createEmptyDraft("Studio"), {
      id: bindingId,
      type: "string",
      required: false,
    });
    if (!variable.ok) throw new Error("expected variable");
    const placed = addInputComponent(variable.draft, bindingId);
    if (!placed.ok) throw new Error("expected placed Input");
    renderHarness(placed.draft);

    const select = screen.getByRole("button", { name: `Select ${bindingId}` });
    expect(select).toHaveAttribute("aria-label", `Select ${bindingId}`);
    fireEvent.click(select);

    const selectedBlock = document.querySelector('[data-selected="true"]');
    expect(selectedBlock?.querySelectorAll("[title]")).toHaveLength(2);
    expect(document.body).toHaveTextContent(bindingId);

    setLocale("ja");
    expect(screen.getByRole("button", { name: `${bindingId}を選択` }))
      .toHaveAttribute("aria-label", `${bindingId}を選択`);
  });

  it("uses the variable ID for variable-bound Text blocks", () => {
    const initial = seedDraft();
    const bindingId = "private_binding_01";
    const variableBound: CompositionDraft = {
      ...initial,
      variables: initial.variables.map((variable) =>
        variable.id === "month" ? { ...variable, id: bindingId } : variable
      ),
      displays: initial.displays.map((display) =>
        display.kind === "text"
          ? {
            ...display,
            content: { kind: "variable" as const, variable: bindingId },
          }
          : display
      ),
    };
    renderHarness(variableBound);

    const select = screen.getByRole("button", { name: `Select ${bindingId}` });
    expect(select).toBeInTheDocument();
    expect(document.body).toHaveTextContent(bindingId);

    setLocale("ja");
    expect(screen.getByRole("button", { name: `${bindingId}を選択` }))
      .toBeInTheDocument();
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
      screen.queryByRole("button", { name: "Move month up" }),
    ).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Select month" }));
    const itemDown = screen.getByRole("button", { name: "Move month down" });
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
      { kind: "component", component: "input-3" },
    ]);

    // Every reorder control is a native button in visual order.
    const names = Array.from(
      container.querySelectorAll(
        ".designRowBar button, .designBlockActions button",
      ),
    ).map((button) => button.getAttribute("aria-label"));
    expect(names).toContain("Move row 1 down");
    expect(names).toContain("Move month up");
    expect(container.querySelectorAll("button[aria-label]")).not.toHaveLength(
      0,
    );
  });

  it("keeps deletion in the selected block Inspector", () => {
    const { container } = renderHarness();
    fireEvent.click(screen.getByRole("button", { name: "Select Monthly" }));

    const blockActions = container.querySelector(
      '[data-block-id="disp-2"] .designBlockActions',
    );
    expect(blockActions?.querySelectorAll("button")).toHaveLength(2);
    expect(blockActions?.querySelector('[aria-label^="Remove "]')).toBeNull();
  });

  it("routes block operations through the draft channel without extra queries", () => {
    renderHarness();

    // Text insert, row move, and selection touch only the draft channel.
    fireEvent.click(
      screen.getAllByRole("button", { name: "Add to design" })[0],
    );
    fireEvent.click(
      screen.getByRole("dialog", { name: "Add to design" }).querySelector(
        ".designPaletteEntries",
      )!.querySelectorAll("button")[0],
    );
    fireEvent.click(screen.getByRole("button", { name: "Move row 2 up" }));
    fireEvent.click(screen.getByRole("button", { name: "Select Summary" }));

    expect(harnessCalls.picker).toHaveLength(0);
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
      container.querySelector(".designCanvas .compositionFlowItem--input"),
    ).not.toBeNull();
    expect(
      container.querySelector(".designCanvas .compositionFlowItem--text"),
    ).not.toBeNull();
    const css = stylesheet();
    // Narrow viewports stack metric and variable blocks while tables keep
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
      "input-3",
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
    ).toContainEqual({
      kind: "component",
      component: "input-3",
    });
  });
});
