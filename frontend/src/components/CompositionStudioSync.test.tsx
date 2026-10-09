import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CompositionStudio } from "./CompositionStudio";
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
} from "~/lib/composition-draft";
import { setLocale } from "~/lib/i18n";

const {
  previewMock,
  querySourceMock,
  canonicalizeMock,
  saveMock,
  sqlGetMock,
  sqlQueryMock,
  sqlListMock,
  formListMock,
  navigateMock,
} = vi.hoisted(() => ({
  previewMock: vi.fn(),
  querySourceMock: vi.fn(),
  canonicalizeMock: vi.fn(),
  saveMock: vi.fn(),
  sqlGetMock: vi.fn(),
  sqlQueryMock: vi.fn(),
  sqlListMock: vi.fn(),
  formListMock: vi.fn(),
  navigateMock: vi.fn(),
}));

vi.mock("@solidjs/router", () => ({
  useLocation: () => ({
    pathname: "/spaces/space-1/compositions/new",
  }),
  useNavigate: () => navigateMock,
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>
      {props.children as never}
    </a>
  ),
}));

vi.mock("~/lib/composition-api", () => ({
  canCreateSavedSqlComposition: (entry: { kind: string }) =>
    entry.kind === "user-query",
  compositionApi: {
    canonicalizeDocument: (...args: unknown[]) =>
      (canonicalizeMock as (...call: unknown[]) => unknown)(...args),
    preview: (...args: unknown[]) =>
      (previewMock as (...call: unknown[]) => unknown)(...args),
    querySource: (...args: unknown[]) =>
      (querySourceMock as (...call: unknown[]) => unknown)(...args),
    save: (...args: unknown[]) =>
      (saveMock as (...call: unknown[]) => unknown)(...args),
  },
}));

vi.mock("~/lib/ugoite-client", () => ({
  formApi: {
    list: (...args: unknown[]) =>
      (formListMock as (...call: unknown[]) => unknown)(...args),
  },
  sqlApi: {
    list: (...args: unknown[]) =>
      (sqlListMock as (...call: unknown[]) => unknown)(...args),
    get: (...args: unknown[]) =>
      (sqlGetMock as (...call: unknown[]) => unknown)(...args),
    query: (...args: unknown[]) =>
      (sqlQueryMock as (...call: unknown[]) => unknown)(...args),
  },
}));

const stylesheet = () => readFileSync(join(__dirname, "..", "app.css"), "utf8");

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** Metric Total on src-1, table Details on src-2, text Summary, month placed. */
const seedDraft = (): CompositionDraft => {
  let draft = createEmptyDraft("Studio");
  draft = addSavedSqlSource(draft, {
    entryId: "sql-1",
    revisionId: "sql-rev-1",
    name: "Monthly totals",
    expectedResult: [{ name: "total", type: "float" as const }],
    variables: {},
  }).draft;
  draft = addEntryQuerySource(draft, {
    formId: "11111111-1111-4111-8111-111111111111",
    name: "Expenses",
    fieldSchema: [{ field_id: 1, field_type: "string" }],
    query: {
      filters: [],
      sort: [],
      projection: { kind: "preview" as const },
    },
  }).draft;
  const month = addParameter(draft, {
    id: "month",
    label: "Month",
    type: "date",
    required: true,
  });
  if (!month.ok) throw new Error("expected month parameter");
  draft = month.draft;
  const metric = addMetricDisplay(draft, "src-1", { column: "total" }, "Total");
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

const sourceOnlyDraft = (): CompositionDraft =>
  addSavedSqlSource(createEmptyDraft("Studio"), {
    entryId: "sql-1",
    revisionId: "sql-rev-1",
    name: "Monthly totals",
    expectedResult: [{ name: "total", type: "float" as const }],
    variables: {},
  }).draft;

const okPreview = (fingerprint: string) => ({
  ok: true,
  draft_fingerprint: fingerprint,
  parameter_definitions: [],
  plan: {
    draft_fingerprint: fingerprint,
    sources: ["src-1", "src-2"].map((id) => ({
      source_id: id,
      request: {},
      source_schema_fingerprint: "fp",
      kind: "entry_query",
    })),
    component_bindings: [],
  },
});

const renderStudio = (initial?: CompositionDraft) =>
  render(() => (
    <CompositionStudio
      spaceId="space-1"
      initialDraft={initial ?? seedDraft()}
      saveMode={{ kind: "create" }}
      backHref="/spaces/space-1/compositions"
      backLabel="Back to compositions"
    />
  ));

const modeRadios = () => {
  const group = screen.getByRole("radiogroup", { name: "Studio mode" });
  return {
    group,
    design: within(group).getByRole("radio", { name: "Design" }),
    data: within(group).getByRole("radio", { name: "Data" }),
    split: within(group).getByRole("radio", { name: "Split" }),
  };
};

describe("CompositionStudioSync", () => {
  beforeEach(() => {
    setLocale("en");
    vi.clearAllMocks();
    canonicalizeMock.mockImplementation(
      async (document: {
        spec: { components: { id: string; label?: string }[] };
      }) => ({
        canonical_yaml: `yaml:${
          document.spec.components.map((component) =>
            `${component.id}=${component.label ?? ""}`
          ).join(",")
        }`,
      }),
    );
    previewMock.mockImplementation(async () => okPreview("fp-1"));
    querySourceMock.mockImplementation(async () => ({
      kind: "entry_query",
      page: { rows: [], has_more: false },
    }));
    sqlGetMock.mockResolvedValue({
      id: "sql-1",
      name: "Monthly totals",
      kind: "user-query",
      sql: "SELECT SUM(amount) AS total FROM expenses",
      variables: [],
      created_at: "2026-10-01T00:00:00Z",
      updated_at: "2026-10-02T00:00:00Z",
      revision_id: "sql-rev-1",
    });
    sqlQueryMock.mockResolvedValue({
      columns: ["total"],
      rows: [[128400]],
      has_more: false,
      result_schema: [{ name: "total", type: "float" }],
    });
    formListMock.mockResolvedValue([
      {
        id: "11111111-1111-4111-8111-111111111111",
        name: "Expenses",
        version: 1,
        template: "task",
        fields: {},
      },
    ]);
    sqlListMock.mockResolvedValue([]);
  });

  afterEach(() => cleanup());

  it("switches the workspace arrangement through one Design Data Split control", () => {
    const { container } = renderStudio();
    const modes = modeRadios();

    // One operative control: icon plus short label per option, natively
    // focusable with an accessible name each.
    expect(modes.group.querySelectorAll("svg")).toHaveLength(3);
    expect(modes.design).toHaveAttribute("aria-checked", "true");

    // Design first: canvas without the Data navigator.
    expect(
      screen.getByRole("button", { name: "Select Total" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Monthly totals" }),
    ).toBeNull();

    fireEvent.click(modes.data);
    expect(modes.data).toHaveAttribute("aria-checked", "true");
    expect(
      screen.getByRole("button", { name: "Monthly totals" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Select Total" }),
    ).toBeNull();

    fireEvent.click(modes.split);
    expect(modes.split).toHaveAttribute("aria-checked", "true");
    expect(
      container.querySelector(".studioSplit"),
    ).not.toBeNull();
    // Split pairs the canvas with the Data pane for the selected block.
    expect(
      screen.getByRole("button", { name: "Select Total" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Monthly totals" }),
    ).toBeInTheDocument();
  });

  it("keeps draft, preview, and selection continuous across modes without refetching", async () => {
    renderStudio();
    await waitFor(() => {
      expect(previewMock).toHaveBeenCalledTimes(1);
    });

    fireEvent.click(screen.getByRole("button", { name: "Select Total" }));
    expect(screen.getByRole("heading", { name: "Metric" })).toBeInTheDocument();

    const modes = modeRadios();
    fireEvent.click(modes.data);
    // The Data selection follows the block without a preview round-trip.
    await screen.findByRole("heading", { name: "Monthly totals" });
    fireEvent.click(modes.split);
    fireEvent.click(modes.design);

    // Selection and draft survive every mode switch.
    expect(screen.getByRole("heading", { name: "Metric" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select Total" }))
      .toHaveAttribute("aria-pressed", "true");

    // Modes switch arrangement only: no state reset, no refetch.
    await sleep(700);
    expect(previewMock).toHaveBeenCalledTimes(1);
    expect(canonicalizeMock).toHaveBeenCalledTimes(1);
  });

  it("auto-selects the block source in Data and keeps text and parameter selections", async () => {
    renderStudio();
    fireEvent.click(modeRadios().split);

    // No source selected yet: the pane shows the navigator only.
    expect(
      screen.queryByRole("heading", { name: "Monthly totals" }),
    ).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Select Total" }));
    await screen.findByRole("heading", { name: "Monthly totals" });

    // Text blocks carry no source: the Data selection is kept, not reset.
    fireEvent.click(screen.getByRole("button", { name: "Select Summary" }));
    expect(
      screen.getByRole("heading", { name: "Monthly totals" }),
    ).toBeInTheDocument();

    // Parameter controls carry no source either.
    fireEvent.click(screen.getByRole("button", { name: "Select Month" }));
    expect(
      screen.getByRole("heading", { name: "Monthly totals" }),
    ).toBeInTheDocument();
  });

  it("soft-highlights using blocks distinctly from the selection", () => {
    const { container } = renderStudio();
    fireEvent.click(modeRadios().split);

    fireEvent.click(screen.getByRole("button", { name: "Expenses" }));
    const table = container.querySelector('[data-block-id="disp-2"]');
    const metric = container.querySelector('[data-block-id="disp-1"]');
    expect(table).toHaveAttribute("data-highlighted");
    expect(metric).not.toHaveAttribute("data-highlighted");
    // Highlight never selects: exactly one selection owner stays empty.
    expect(
      container.querySelectorAll(".designBlock[data-selected]"),
    ).toHaveLength(0);

    fireEvent.click(screen.getByRole("button", { name: "Select Total" }));
    // Block selection auto-selects its own source, so the highlight follows
    // the Data selection while the selection owner stays the block.
    expect(metric).toHaveAttribute("data-selected");
    expect(metric).toHaveAttribute("data-highlighted");
    expect(table).not.toHaveAttribute("data-selected");
    expect(table).not.toHaveAttribute("data-highlighted");
  });

  it("resolves block, source, and mode changes during an in-flight preview to the latest draft only", async () => {
    let resolveFirst!: (response: unknown) => void;
    previewMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveFirst = resolve;
        }),
    );
    previewMock.mockImplementation(async () => okPreview("fp-new"));
    renderStudio();
    await sleep(600);
    // The first preview is still in flight.
    expect(previewMock).toHaveBeenCalledTimes(1);

    // Sync transitions during the flight never trigger a preview.
    const modes = modeRadios();
    fireEvent.click(modes.split);
    fireEvent.click(modes.data);
    fireEvent.click(modes.design);
    fireEvent.click(screen.getByRole("button", { name: "Select Summary" }));
    expect(previewMock).toHaveBeenCalledTimes(1);

    // A draft edit schedules exactly one follow-up preview. The inspector
    // label field is scoped: the Display section renders label inputs too.
    fireEvent.click(screen.getByRole("button", { name: "Select Total" }));
    const inspector = screen.getByRole("complementary");
    fireEvent.input(within(inspector).getByLabelText("Label"), {
      target: { value: "Total v2" },
    });
    await waitFor(() => {
      expect(previewMock).toHaveBeenCalledTimes(2);
    });
    const latestYaml = (previewMock.mock.calls[1] as unknown[])[1];
    expect(latestYaml).toContain("Total v2");

    // The stale first response resolves last and never resurrects.
    resolveFirst(okPreview("fp-old"));
    await sleep(200);
    expect(previewMock).toHaveBeenCalledTimes(2);
  });

  it("separates Data sources, Parameters, and Tags into tabs in Data mode", () => {
    const { container } = renderStudio();
    const modes = modeRadios();
    // Top-level studio sections only: canvas block content carries its own
    // headings, so section chrome is counted from the studio root.
    const topSectionCount = () =>
      Array.from(container.firstElementChild?.children ?? []).filter(
        (element) => element.tagName === "SECTION",
      ).length;

    // Design renders the finished shape only: canvas plus inspector, no
    // Data workspace, no Parameters, no Tags, no Preview section.
    expect(screen.getByRole("heading", { name: "Design" })).toBeInTheDocument();
    expect(topSectionCount()).toBe(1);
    expect(
      screen.getByRole("button", { name: "Select Total" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Monthly totals" }),
    ).toBeNull();
    expect(
      screen.queryByRole("heading", { name: "Parameters" }),
    ).toBeNull();
    expect(screen.queryByRole("heading", { name: "Tags" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "Preview" })).toBeNull();

    // Data renders only the active fetch-definition panel. Parameters and
    // Tags are separate tabs so the source editor stays the initial focus.
    fireEvent.click(modes.data);
    expect(screen.getByRole("heading", { name: "Data" })).toBeInTheDocument();
    const dataTabs = screen.getByRole("tablist", { name: "Data" });
    expect(within(dataTabs).getAllByRole("tab").map((tab) => tab.textContent))
      .toEqual(["Sources", "Parameters", "Tags"]);
    expect(
      within(dataTabs).getAllByRole("tab").every((tab) =>
        (tab as HTMLButtonElement).tabIndex === 0
      ),
    ).toBe(true);
    expect(within(dataTabs).getByRole("tab", { name: "Sources" }))
      .toHaveAttribute("aria-selected", "true");
    expect(topSectionCount()).toBe(1);
    expect(
      screen.getByRole("button", { name: "Monthly totals" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Parameters" }),
    ).toBeNull();
    expect(screen.queryByRole("heading", { name: "Tags" })).toBeNull();
    fireEvent.click(within(dataTabs).getByRole("tab", { name: "Parameters" }));
    expect(screen.getByRole("heading", { name: "Parameters" }))
      .toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Tags" })).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Monthly totals" }),
    ).toBeNull();
    fireEvent.click(within(dataTabs).getByRole("tab", { name: "Tags" }));
    expect(screen.getByRole("heading", { name: "Tags" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Parameters" })).toBeNull();
    fireEvent.click(within(dataTabs).getByRole("tab", { name: "Sources" }));
    expect(
      screen.getByRole("button", { name: "Monthly totals" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Select Total" }),
    ).toBeNull();
    expect(screen.queryByRole("heading", { name: "Preview" })).toBeNull();

    // Split pairs the canvas with the Data pane and nothing else:
    // parameters and tags edit in Data mode.
    fireEvent.click(modes.split);
    expect(screen.getByRole("heading", { name: "Design" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Data" })).toBeInTheDocument();
    expect(topSectionCount()).toBe(1);
    expect(
      screen.getByRole("button", { name: "Select Total" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Monthly totals" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Parameters" }),
    ).toBeNull();
    expect(screen.queryByRole("heading", { name: "Tags" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "Preview" })).toBeNull();
  });

  it("guides a blank canvas with one Add-data action that opens the single picker", async () => {
    const { container } = renderStudio(createEmptyDraft("Blank"));

    // One structural action, no prose paragraphs, no canvas gaps.
    expect(
      screen.getAllByRole("heading", { level: 2 }).map((heading) =>
        heading.textContent
      ),
    ).toEqual(["Design"]);
    expect(container.querySelectorAll("p")).toHaveLength(0);
    expect(
      screen.getAllByRole("button", { name: "Add data" }),
    ).toHaveLength(1);
    expect(
      screen.queryByRole("button", { name: "Add block" }),
    ).toBeNull();

    // The action reuses the existing Add-data label and lifts the single
    // source picker dialog; picking a source returns the normal canvas.
    fireEvent.click(screen.getByRole("button", { name: "Add data" }));
    expect(
      await screen.findByRole("dialog", { name: "Add data" }),
    ).toBeInTheDocument();
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect(
      screen.getAllByRole("button", { name: "Add data" }),
    ).toHaveLength(1);
    expect(
      screen.getAllByRole("button", { name: "Add block" }).length,
    ).toBeGreaterThan(0);
    expect(
      await screen.findByRole("button", { name: "Select Expenses" }),
    ).toBeInTheDocument();
  });

  it("blocks a source-only draft before canonicalization", () => {
    renderStudio(sourceOnlyDraft());

    const saveButton = screen.getByRole("button", {
      name: "Save, Add a block to the canvas to save.",
    });
    expect(saveButton).toBeDisabled();
    expect(saveButton).toHaveAttribute(
      "title",
      "Save, Add a block to the canvas to save.",
    );
    fireEvent.click(saveButton);
    expect(canonicalizeMock).not.toHaveBeenCalled();
    expect(saveMock).not.toHaveBeenCalled();
  });

  it("adds a Saved SQL source and its table directly from Design", async () => {
    sqlListMock.mockResolvedValue([{
      id: "sql-1",
      name: "Monthly totals",
      kind: "user-query",
      sql: "SELECT SUM(amount) AS total FROM expenses",
      variables: [],
      created_at: "2026-10-01T00:00:00Z",
      updated_at: "2026-10-02T00:00:00Z",
      revision_id: "sql-rev-1",
    }]);
    const { container } = renderStudio(createEmptyDraft("Blank"));
    fireEvent.click(screen.getByRole("button", { name: "Add data" }));
    expect(screen.getByRole("dialog", { name: "Add data" }))
      .toBeInTheDocument();
    fireEvent.click(await screen.findByRole("tab", { name: "Saved SQL" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Monthly totals" }),
    );

    expect(
      await screen.findByRole("button", { name: "Select Monthly totals" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "Add data" })).toBeNull();
    expect(container.querySelectorAll(".designBlock")).toHaveLength(1);
  });

  it("puts current sources first and reuses one from Design", async () => {
    renderStudio();
    fireEvent.click(screen.getByRole("button", { name: "Add data" }));

    const currentSources = await screen.findByRole("heading", {
      name: "In this composition",
    });
    const currentSourceGroup = currentSources.parentElement;
    if (!currentSourceGroup) throw new Error("expected current source group");
    fireEvent.click(
      within(currentSourceGroup).getByRole("button", {
        name: "Expenses, Forms",
      }),
    );
    expect(screen.queryByRole("dialog", { name: "Add data" })).toBeNull();
    expect(
      await screen.findByRole("button", { name: "Select Expenses" }),
    ).toBeInTheDocument();

    fireEvent.click(modeRadios().data);
    expect(screen.queryByRole("button", { name: "Add data" })).toBeNull();
    expect(screen.getAllByRole("button", { name: "Expenses" })).toHaveLength(1);
  });

  it("keeps one source action in Design and none in Data details", () => {
    renderStudio();
    expect(screen.getAllByRole("button", { name: "Add data" })).toHaveLength(1);

    fireEvent.click(modeRadios().data);
    expect(screen.queryByRole("button", { name: "Add data" })).toBeNull();

    fireEvent.click(modeRadios().split);
    expect(screen.getAllByRole("button", { name: "Add data" })).toHaveLength(1);
  });

  it("keeps a single Add-data control for a blank draft in Split", async () => {
    renderStudio(createEmptyDraft("Blank"));
    fireEvent.click(
      screen.getByRole("radio", { name: "Split", exact: true }),
    );
    // The canvas fallback owns the action; the pane button hides so the
    // same name never appears twice.
    expect(
      screen.getAllByRole("button", { name: "Add data" }),
    ).toHaveLength(1);
  });

  it("renders resolve diagnostics in the strip in every mode without refetch", async () => {
    previewMock.mockImplementation(async () => ({
      ok: false,
      draft_fingerprint: "fp-bad",
      diagnostics: [{ code: "source_unavailable" }],
    }));
    renderStudio();
    const modes = modeRadios();

    const stripAlert = () => screen.getByRole("alert", { name: "Diagnostics" });
    await waitFor(() => {
      expect(stripAlert()).toHaveTextContent("A data source is unavailable.");
    });
    // The removed Preview section is gone; the strip retry stays.
    expect(
      screen.getByRole("button", { name: "Retry preview" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Preview" })).toBeNull();

    // Mode switches keep the shared diagnostics with no preview round-trip.
    fireEvent.click(modes.data);
    expect(stripAlert()).toHaveTextContent("A data source is unavailable.");
    fireEvent.click(modes.split);
    expect(stripAlert()).toHaveTextContent("A data source is unavailable.");
    fireEvent.click(modes.design);
    expect(stripAlert()).toHaveTextContent("A data source is unavailable.");

    await sleep(700);
    expect(previewMock).toHaveBeenCalledTimes(1);
  });

  it("saves through one visible text button with the blocked reason in its name", () => {
    renderStudio(createEmptyDraft(""));
    // Single control: visible text, never an icon-only button.
    const save = screen.getByRole("button", {
      name: "Save, Enter a name to save.",
    });
    expect(save).toBeDisabled();
    expect(save).toHaveAttribute("title", "Save, Enter a name to save.");
    expect(save.textContent).toBe("Save");
    expect(save.querySelector("svg")).toBeNull();
  });

  it("keeps the steady Save label while the save is in flight", async () => {
    let resolveSave!: (response: unknown) => void;
    saveMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveSave = resolve;
        }),
    );
    renderStudio();
    const save = screen.getByRole("button", { name: "Save" });
    expect(save).toBeEnabled();
    expect(save.textContent).toBe("Save");

    fireEvent.click(save);
    // Explicit busy without layout shift: the same label, disabled, still
    // the single Save control.
    await waitFor(() => {
      expect(save).toBeDisabled();
    });
    expect(save.textContent).toBe("Save");
    expect(screen.getByRole("button", { name: "Save" })).toBe(save);

    await sleep(700);
    expect(saveMock).toHaveBeenCalledTimes(1);
    resolveSave({ composition_id: "tool-1", revision_id: "rev-1" });
    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledTimes(1);
    });
    expect(save).toBeEnabled();
  });

  it("fits the destination-named back link, name, and Save in one 390px header row", () => {
    const { container } = renderStudio();
    const header = container.querySelector("header");
    expect(header).not.toBeNull();
    const scope = within(header as HTMLElement);
    // One icon-only parent link, one name input, one Save text button.
    expect(scope.getByRole("link", { name: "Back to compositions" }))
      .toHaveClass("pill", "iconpill", "icononly", "back-link");
    expect(header?.firstElementChild).toBe(
      scope.getByRole("link", { name: "Back to compositions" }),
    );
    const name = scope.getByLabelText("Name");
    expect(name).toHaveClass("ui-input");
    expect(scope.getByRole("button", { name: "Save" })).toHaveTextContent(
      "Save",
    );
    // The name input flexes: it shrinks inside the one-row flex header, so
    // the text Save never pushes the row past 390px.
    expect(header).toHaveClass("flex");
    expect(stylesheet()).toMatch(/\.ui-input[\s\S]*?min-width:\s*0/);
  });

  it("keeps Split desktop-only with stacked panes at narrow widths", () => {
    const css = stylesheet();
    // The Split option hides below the desktop width: narrow viewports get
    // Design | Data only.
    expect(css).toMatch(
      /@media\s*\(max-width:\s*760px\)[\s\S]*?\.studioModeSplit\s*\{\s*display:\s*none/,
    );
    // Split pairs canvas and pane side by side, stacking to one column
    // below the desktop width; the pane reuses the workspace narrowed.
    expect(css).toMatch(
      /\.studioSplit\s*\{[^}]*grid-template-columns:\s*minmax\(0,\s*8fr\)/,
    );
    expect(css).toMatch(
      /@media\s*\(max-width:\s*760px\)[\s\S]*?\.studioSplit\s*\{[\s\S]*?grid-template-columns:\s*minmax\(0,\s*1fr\)/,
    );
    expect(css).toMatch(
      /\.studioSplitPane\s+\.dataWorkspace\s*\{[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\)/,
    );
    // Source highlight rides token colors, distinct from the selection.
    expect(css).toMatch(
      /\.designBlock\[data-highlighted\]:not\(\[data-selected\]\)/,
    );
  });
});
