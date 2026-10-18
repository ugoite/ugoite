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
  addInlineSqlSource,
  addInputComponent,
  addMetricDisplay,
  addSavedSqlSource,
  addTableDisplay,
  addTextDisplay,
  addVariable,
  type CompositionDraft,
  createEmptyDraft,
  removeInputComponent,
  setMetricSource,
} from "~/lib/composition-draft";
import { setLocale } from "~/lib/i18n";

const {
  previewMock,
  queryMetricMock,
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
  queryMetricMock: vi.fn(),
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
  compositionResultTypeFromSql: (resultType: string | undefined) =>
    ["string", "boolean", "integer", "float", "date", "timestamp", "json"]
        .includes(resultType ?? "")
      ? resultType
      : "json",
  canCreateSavedSqlComposition: (entry: { kind: string }) =>
    entry.kind === "user-query",
  compositionApi: {
    canonicalizeDocument: (...args: unknown[]) =>
      (canonicalizeMock as (...call: unknown[]) => unknown)(...args),
    preview: (...args: unknown[]) =>
      (previewMock as (...call: unknown[]) => unknown)(...args),
    queryMetric: (...args: unknown[]) =>
      (queryMetricMock as (...call: unknown[]) => unknown)(...args),
    querySource: (...args: unknown[]) =>
      (querySourceMock as (...call: unknown[]) => unknown)(...args),
    save: (...args: unknown[]) =>
      (saveMock as (...call: unknown[]) => unknown)(...args),
  },
}));

vi.mock("~/lib/ugoite-client", () => ({
  entryApi: {
    getRevision: vi.fn().mockResolvedValue({
      fields: { variables: [] },
    }),
  },
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
const blockButton = (draftId: string): HTMLButtonElement | null =>
  document.querySelector(
    `[data-block-id="${draftId}"] button[aria-label^="Select "]`,
  );
const requireBlockButton = (draftId: string): HTMLButtonElement => {
  const button = blockButton(draftId);
  if (!button) throw new Error(`Expected canvas block ${draftId}`);
  return button;
};
const waitForBlockButton = async (draftId: string) => {
  await waitFor(() => expect(blockButton(draftId)).not.toBeNull());
  return requireBlockButton(draftId);
};

/** Metric Total on src-1, table Details on src-2, text Summary, month placed. */
const seedDraft = (): CompositionDraft => {
  let draft = createEmptyDraft("Studio");
  draft = addSavedSqlSource(draft, {
    entryId: "sql-1",
    revisionId: "sql-rev-1",
    name: "Monthly totals",
    expectedResult: [{ name: "total", type: "float" as const }],
    variables: {},
    variableTypes: {},
  }).draft;
  draft = addInlineSqlSource(draft, {
    name: "Expenses",
    sql: 'SELECT "Amount" AS total FROM "Expenses"',
    formBindings: [{ relation: "Expenses", form_id: "form-1" }],
    expectedResult: [{ name: "total", type: "float" as const }],
    schemaFingerprint: "a".repeat(64),
  }).draft;
  const month = addVariable(draft, {
    id: "month",
    type: "date",
    required: true,
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
  const placed = addInputComponent(draft, "month");
  if (!placed.ok) throw new Error("expected variable placement");
  return placed.draft;
};

const requiredVariableWithoutPlacedInputDraft = (): CompositionDraft => {
  const draft = seedDraft();
  const input = draft.inputs[0];
  if (!input) throw new Error("expected placed Input");
  const unplaced = removeInputComponent(draft, input.draftId);
  if (!unplaced.ok) throw new Error("expected Input unplacement");
  return unplaced.draft;
};

const sourceOnlyDraft = (): CompositionDraft =>
  addSavedSqlSource(createEmptyDraft("Studio"), {
    entryId: "sql-1",
    revisionId: "sql-rev-1",
    name: "Monthly totals",
    expectedResult: [{ name: "total", type: "float" as const }],
    variables: {},
    variableTypes: {},
  }).draft;

const sqlMetricDraft = (): CompositionDraft => {
  const primary = addSavedSqlSource(createEmptyDraft("Studio"), {
    entryId: "sql-1",
    revisionId: "sql-rev-1",
    name: "Monthly totals",
    expectedResult: [{ name: "total", type: "float" as const }],
    variables: {},
    variableTypes: {},
  });
  const alternate = addSavedSqlSource(primary.draft, {
    entryId: "sql-2",
    revisionId: "sql-rev-2",
    name: "Yearly totals",
    expectedResult: [{ name: "total", type: "float" as const }],
    variables: {},
    variableTypes: {},
  });
  const metric = addMetricDisplay(alternate.draft, "src-1", "total");
  if (!metric.ok) throw new Error("expected Saved SQL Metric");
  return metric.draft;
};

const incompleteSavedSqlMetricDraft = (): CompositionDraft => {
  const moved = setMetricSource(sqlMetricDraft(), "disp-1", "src-2");
  if (!moved.ok) throw new Error("expected incomplete Saved SQL Metric");
  return moved.draft;
};

const okPreview = (fingerprint: string) => ({
  ok: true,
  draft_fingerprint: fingerprint,
  variable_definitions: [],
  plan: {
    draft_fingerprint: fingerprint,
    sources: ["src-1", "src-2"].map((id) => ({
      source_id: id,
      request: {},
      source_schema_fingerprint: "fp",
      kind: id === "src-1" ? "saved_sql" : "inline_sql",
      expected_result: [{ name: "total", type: "float" }],
      ...(id === "src-2"
        ? { form_bindings: [{ relation: "Expenses", form_id: "form-1" }] }
        : {}),
    })),
    component_bindings: [],
  },
});

const okMetricPreview = (
  fingerprint: string,
  variableConsumers: Record<string, { source_ids: string[] }> = {},
) => ({
  ok: true,
  draft_fingerprint: fingerprint,
  variable_definitions: [],
  plan: {
    draft_fingerprint: fingerprint,
    sources: [{
      source_id: "src-1",
      request: {},
      source_schema_fingerprint: "fp",
      kind: "saved_sql",
      expected_result: [{ name: "total", type: "float" }],
    }],
    component_bindings: [{
      component_id: "disp-1",
      kind: "metric",
      source_id: "src-1",
      expected_result_type: "float",
    }],
    dependencies: { variable_consumers: variableConsumers },
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
        spec: { components: unknown[] };
      }) => ({
        canonical_yaml: `yaml:${JSON.stringify(document.spec.components)}`,
        fingerprint: "canonical-fingerprint",
      }),
    );
    previewMock.mockImplementation(async () => okPreview("fp-1"));
    querySourceMock.mockImplementation(async () => ({
      kind: "sql",
      page: { columns: [], rows: [], has_more: false },
    }));
    queryMetricMock.mockResolvedValue({
      ok: true,
      draft_fingerprint: "fp-studio",
      result: {
        component_id: "disp-1",
        result_type: "float",
        value: 128400,
      },
    });
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

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("switches the workspace arrangement through one Design Data Split control", () => {
    const { container } = renderStudio();
    const modes = modeRadios();

    // One operative control: icon plus short label per option, natively
    // focusable with an accessible name each.
    expect(modes.group.querySelectorAll("svg")).toHaveLength(3);
    expect(modes.design).toHaveAttribute("aria-checked", "true");

    // Design first: canvas without the Data navigator.
    expect(
      requireBlockButton("disp-1"),
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
      blockButton("disp-1"),
    ).toBeNull();

    fireEvent.click(modes.split);
    expect(modes.split).toHaveAttribute("aria-checked", "true");
    expect(
      container.querySelector(".studioSplit"),
    ).not.toBeNull();
    // Split pairs the canvas with the Data pane for the selected block.
    expect(
      requireBlockButton("disp-1"),
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
    await waitFor(() => {
      expect(previewMock).toHaveBeenCalledTimes(2);
    });
    const previewCount = previewMock.mock.calls.length;
    const canonicalizeCount = canonicalizeMock.mock.calls.length;

    fireEvent.click(requireBlockButton("disp-1"));
    expect(screen.getByRole("heading", { name: "Metric" })).toBeInTheDocument();

    const modes = modeRadios();
    fireEvent.click(modes.data);
    // The Data selection follows the block without a preview round-trip.
    expect(screen.getByRole("button", { name: "Monthly totals" }))
      .toHaveAttribute("aria-pressed", "true");
    fireEvent.click(modes.split);
    fireEvent.click(modes.design);

    // Selection and draft survive every mode switch.
    expect(screen.getByRole("heading", { name: "Metric" })).toBeInTheDocument();
    expect(requireBlockButton("disp-1"))
      .toHaveAttribute("aria-pressed", "true");

    // Modes switch arrangement only: no state reset, no refetch.
    await sleep(700);
    expect(previewMock).toHaveBeenCalledTimes(previewCount);
    expect(canonicalizeMock).toHaveBeenCalledTimes(canonicalizeCount);
  });

  it("queries a typed draft metric with its exact preview and current variable values", async () => {
    previewMock.mockImplementationOnce(async () =>
      okMetricPreview("fp-studio", { month: { source_ids: ["src-1"] } })
    );
    const draft = seedDraft();
    const withVariableDefault = {
      ...draft,
      sources: draft.sources.map((source) =>
        source.kind === "saved_sql" && source.draftId === "src-1"
          ? { ...source, variables: { period: { variable: "month" } } }
          : source
      ),
      variables: draft.variables.map((variable) => ({
        ...variable,
        default: variable.id === "month" ? "2026-10-01" : variable.default,
      })),
    };

    renderStudio(withVariableDefault);

    await waitFor(() => {
      expect(queryMetricMock).toHaveBeenCalledTimes(1);
    });
    const previewYaml = (previewMock.mock.calls[0] as unknown[])[1];
    expect(queryMetricMock).toHaveBeenCalledWith(
      "space-1",
      {
        target: { kind: "draft", yaml: previewYaml },
        component_id: "disp-1",
        variables: { month: "2026-10-01" },
      },
      expect.any(AbortSignal),
    );
    // A matching server fingerprint is required before the draft result is shown.
    expect(
      await screen.findByText("128400", { selector: "output" }),
    ).toBeInTheDocument();
  });

  it("rejects a successful metric preview with a different draft fingerprint", async () => {
    queryMetricMock.mockResolvedValueOnce({
      ok: true,
      draft_fingerprint: "stale-fingerprint",
      result: {
        component_id: "disp-1",
        result_type: "float",
        value: 128400,
      },
    });
    const { container } = renderStudio(sourceOnlyDraft());
    fireEvent.click(screen.getByRole("button", { name: "Add to design" }));
    fireEvent.click(
      within(screen.getByRole("dialog", { name: "Add to design" }))
        .getByRole("button", { name: "Data" }),
    );

    const picker = screen.getByRole("dialog", { name: "Add data component" });
    fireEvent.click(within(picker).getByRole("tab", { name: "Metric" }));
    fireEvent.click(
      within(picker).getByRole("button", { name: "Monthly totals" }),
    );
    fireEvent.change(within(picker).getByLabelText("Column"), {
      target: { value: "total" },
    });
    fireEvent.click(within(picker).getByRole("button", { name: "Preview" }));

    expect(await within(picker).findByRole("alert")).toHaveTextContent(
      "Metric result unavailable",
    );
    expect(within(picker).getByRole("button", { name: "Add" }))
      .toBeDisabled();
    expect(container.querySelectorAll(".designBlock")).toHaveLength(0);
    expect(queryMetricMock).toHaveBeenCalledWith(
      "space-1",
      expect.objectContaining({
        target: { kind: "draft", yaml: expect.any(String) },
        component_id: expect.any(String),
      }),
    );
  });

  it("auto-selects the block source in Data and keeps text and variable selections", async () => {
    renderStudio();
    fireEvent.click(modeRadios().split);

    // No source selected yet: the pane shows the navigator only.
    expect(screen.getByRole("button", { name: "Monthly totals" }))
      .toHaveAttribute("aria-pressed", "false");

    fireEvent.click(requireBlockButton("disp-1"));
    expect(screen.getByRole("button", { name: "Monthly totals" }))
      .toHaveAttribute("aria-pressed", "true");

    // Text blocks carry no source: the Data selection is kept, not reset.
    fireEvent.click(requireBlockButton("disp-3"));
    expect(
      screen.getByRole("button", { name: "Monthly totals" }),
    ).toHaveAttribute("aria-pressed", "true");

    // Variable controls carry no source either.
    fireEvent.click(requireBlockButton("input-4"));
    expect(
      screen.getByRole("button", { name: "Monthly totals" }),
    ).toHaveAttribute("aria-pressed", "true");
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

    fireEvent.click(requireBlockButton("disp-1"));
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
    fireEvent.click(requireBlockButton("disp-3"));
    expect(previewMock).toHaveBeenCalledTimes(1);

    // A persisted text edit schedules exactly one follow-up preview.
    const inspector = screen.getByRole("complementary");
    fireEvent.input(within(inspector).getByLabelText("Text body"), {
      target: { value: "Summary v2" },
    });
    await waitFor(() => {
      expect(previewMock).toHaveBeenCalledTimes(2);
    });
    const latestYaml = (previewMock.mock.calls[1] as unknown[])[1];
    expect(latestYaml).toContain("Summary v2");

    // The stale first response resolves last and never resurrects.
    resolveFirst(okPreview("fp-old"));
    await sleep(200);
    expect(previewMock).toHaveBeenCalledTimes(2);
  });

  it("separates Data sources and Variables into tabs in Data mode", () => {
    const { container } = renderStudio();
    const modes = modeRadios();
    // Top-level studio sections only: canvas block content carries its own
    // headings, so section chrome is counted from the studio root.
    const topSectionCount = () =>
      Array.from(container.firstElementChild?.children ?? []).filter(
        (element) => element.tagName === "SECTION",
      ).length;

    // Design renders the finished shape only: canvas plus inspector, no
    // Data workspace, no Variables, no Preview section.
    expect(screen.getByRole("radio", { name: "Design" }))
      .toHaveAttribute("aria-checked", "true");
    expect(screen.queryByRole("heading", { name: "Design" })).toBeNull();
    expect(topSectionCount()).toBe(1);
    expect(
      requireBlockButton("disp-1"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Monthly totals" }),
    ).toBeNull();
    expect(
      screen.queryByRole("heading", { name: "Variables" }),
    ).toBeNull();
    expect(screen.queryByRole("heading", { name: "Preview" })).toBeNull();

    // Data renders only the active fetch-definition panel. Variables and
    // Sources are separate tabs so the source editor stays the initial focus.
    fireEvent.click(modes.data);
    expect(modes.data).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByRole("heading", { name: "Data" })).toBeNull();
    const dataTabs = screen.getByRole("tablist", { name: "Data" });
    expect(within(dataTabs).getAllByRole("tab").map((tab) => tab.textContent))
      .toEqual(["Sources", "Variables"]);
    expect(within(dataTabs).queryByRole("tab", { name: "Tags" })).toBeNull();
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
      screen.queryByRole("heading", { name: "Variables" }),
    ).toBeNull();
    fireEvent.click(within(dataTabs).getByRole("tab", { name: "Variables" }));
    expect(screen.queryByRole("heading", { name: "Variables" })).toBeNull();
    expect(screen.getByRole("tabpanel", { name: "Variables" }))
      .toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Monthly totals" }),
    ).toBeNull();
    fireEvent.click(within(dataTabs).getByRole("tab", { name: "Sources" }));
    expect(
      screen.getByRole("button", { name: "Monthly totals" }),
    ).toBeInTheDocument();
    expect(
      blockButton("disp-1"),
    ).toBeNull();
    expect(screen.queryByRole("heading", { name: "Preview" })).toBeNull();

    // Split pairs the canvas with the Data pane and nothing else:
    // Variables edit in Data mode.
    fireEvent.click(modes.split);
    expect(screen.getByRole("radio", { name: "Split" }))
      .toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("region", { name: "Design" }))
      .toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Data" }))
      .toBeInTheDocument();
    expect(topSectionCount()).toBe(1);
    expect(
      requireBlockButton("disp-1"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Monthly totals" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Variables" }),
    ).toBeNull();
    expect(screen.queryByRole("heading", { name: "Preview" })).toBeNull();
  });

  it("opens Text or Data insertion from a blank canvas", async () => {
    const { container } = renderStudio(createEmptyDraft("Blank"));

    // One structural add control, no prose paragraphs.
    expect(screen.queryAllByRole("heading", { level: 2 })).toHaveLength(0);
    expect(screen.getByRole("region", { name: "Design" }))
      .toBeInTheDocument();
    expect(container.querySelectorAll("p")).toHaveLength(0);
    expect(
      screen.getAllByRole("button", { name: "Add to design" }),
    ).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Add to design" }));
    const palette = screen.getByRole("dialog", { name: "Add to design" });
    expect(within(palette).getByRole("button", { name: "Text" }))
      .toBeInTheDocument();
    fireEvent.click(within(palette).getByRole("button", { name: "Data" }));

    const dataPicker = screen.getByRole("dialog", {
      name: "Add data component",
    });
    fireEvent.click(
      within(dataPicker).getByRole("button", {
        name: "Choose a Form or Saved SQL",
      }),
    );
    expect(await screen.findByRole("dialog", { name: "Choose a source" }))
      .toBeInTheDocument();
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect(
      screen.getAllByRole("button", { name: "Add to design" }),
    ).toHaveLength(2);
    expect(
      await waitForBlockButton("disp-1"),
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

  it("blocks saving a required variable until its Input is placed", () => {
    renderStudio(requiredVariableWithoutPlacedInputDraft());

    const save = screen.getByRole("button", {
      name: "Save, Resolve invalid references to save.",
    });
    expect(save).toBeDisabled();
    expect(save).toHaveAttribute(
      "title",
      "Save, Resolve invalid references to save.",
    );
    fireEvent.click(save);
    expect(saveMock).not.toHaveBeenCalled();
  });

  it("disables Save when a retargeted Metric needs a new SQL column", async () => {
    renderStudio(sqlMetricDraft());
    const save = screen.getByRole("button", { name: "Save" });
    expect(save).toBeEnabled();

    fireEvent.click(
      await waitForBlockButton("disp-1"),
    );
    fireEvent.change(screen.getByRole("combobox", { name: "Source" }), {
      target: { value: "src-2" },
    });

    const blockedSave = screen.getByRole("button", {
      name: "Save, Complete the metric settings to save.",
    });
    expect(blockedSave).toBeDisabled();
    fireEvent.click(blockedSave);
    expect(canonicalizeMock).not.toHaveBeenCalled();
    expect(saveMock).not.toHaveBeenCalled();
  });

  it("skips canonicalization and clears preview when Metric source changes", async () => {
    vi.useFakeTimers();
    previewMock.mockImplementationOnce(async () =>
      okMetricPreview("fp-studio")
    );
    renderStudio(sqlMetricDraft());

    // The preview debounce is 400ms and the Metric request debounce is 120ms.
    await vi.advanceTimersByTimeAsync(521);
    expect(canonicalizeMock).toHaveBeenCalledTimes(1);
    expect(previewMock).toHaveBeenCalledTimes(1);
    expect(queryMetricMock).toHaveBeenCalledTimes(1);
    expect(screen.getByText("128400", { selector: "output" }))
      .toBeInTheDocument();

    fireEvent.click(requireBlockButton("disp-1"));
    fireEvent.change(screen.getByRole("combobox", { name: "Source" }), {
      target: { value: "src-2" },
    });

    // Include the post-effect scheduling margin used by the resolved-preview
    // case above, then leave canonicalization pending while the Metric changes.
    await vi.advanceTimersByTimeAsync(521);
    expect(canonicalizeMock).toHaveBeenCalledTimes(1);
    expect(previewMock).toHaveBeenCalledTimes(1);
    expect(queryMetricMock).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("128400", { selector: "output" }))
      .not.toBeInTheDocument();
  });

  it("does not preview a canonicalization that resolves after a Metric becomes incomplete", async () => {
    vi.useFakeTimers();
    let finishCanonicalize!: (value: { canonical_yaml: string }) => void;
    canonicalizeMock.mockImplementationOnce(
      () => new Promise((resolve) => finishCanonicalize = resolve),
    );
    renderStudio(sqlMetricDraft());

    // Let the initial Solid effects schedule the debounce before asserting
    // that canonicalization has started; this keeps the test independent of
    // when the render's effects flush under CI load.
    await vi.advanceTimersByTimeAsync(521);
    expect(canonicalizeMock).toHaveBeenCalledTimes(1);
    expect(previewMock).not.toHaveBeenCalled();

    fireEvent.click(requireBlockButton("disp-1"));
    fireEvent.change(screen.getByRole("combobox", { name: "Source" }), {
      target: { value: "src-2" },
    });
    finishCanonicalize({ canonical_yaml: "yaml:stale" });
    await Promise.resolve();
    await Promise.resolve();

    expect(canonicalizeMock).toHaveBeenCalledTimes(1);
    expect(previewMock).not.toHaveBeenCalled();
    expect(queryMetricMock).not.toHaveBeenCalled();
  });

  it("disables Save when a Saved SQL Metric has no output column", () => {
    renderStudio(incompleteSavedSqlMetricDraft());

    const save = screen.getByRole("button", {
      name: "Save, Complete the metric settings to save.",
    });
    expect(save).toBeDisabled();
    fireEvent.click(save);
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
    fireEvent.click(screen.getByRole("button", { name: "Add to design" }));
    fireEvent.click(
      screen.getByRole("dialog", { name: "Add to design" }).querySelector(
        ".designPaletteItem:nth-child(2)",
      )!,
    );
    const dataPicker = screen.getByRole("dialog", {
      name: "Add data component",
    });
    fireEvent.click(
      within(dataPicker).getByRole("button", {
        name: "Choose a Form or Saved SQL",
      }),
    );
    expect(screen.getByRole("dialog", { name: "Choose a source" }))
      .toBeInTheDocument();
    fireEvent.click(await screen.findByRole("tab", { name: "Saved SQL" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Monthly totals" }),
    );

    expect(
      await waitForBlockButton("disp-1"),
    ).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "Choose a source" }))
      .toBeNull();
    expect(container.querySelectorAll(".designBlock")).toHaveLength(1);
  });

  it("puts current sources first and reuses one from Design", async () => {
    const { container } = renderStudio();
    fireEvent.click(
      screen.getAllByRole("button", { name: "Add to design" })[0],
    );
    fireEvent.click(
      screen.getByRole("dialog", { name: "Add to design" }).querySelector(
        ".designPaletteItem:nth-child(2)",
      )!,
    );
    const dataPicker = screen.getByRole("dialog", {
      name: "Add data component",
    });
    fireEvent.click(
      within(dataPicker).getByRole("button", {
        name: "Choose a Form or Saved SQL",
      }),
    );
    const sourcePicker = screen.getByRole("dialog", { name: "Choose a source" });
    const currentSources = await within(sourcePicker).findByRole("list", {
      name: "In this composition",
    });
    const currentExpense = within(currentSources).getByRole("button", {
      name: /Expenses/,
    });
    expect(currentExpense).toBeInTheDocument();
    fireEvent.click(currentExpense);
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Choose a source" }))
        .toBeNull();
    });
    expect(container.querySelectorAll('[data-block-id^="disp-"]'))
      .toHaveLength(4);

    fireEvent.click(modeRadios().data);
    expect(screen.queryByRole("button", { name: "Add data" })).toBeNull();
    expect(screen.getAllByRole("button", { name: "Expenses" })).toHaveLength(1);
    expect(
      container.querySelectorAll(".dataWorkspaceNavigator .rowListItem"),
    ).toHaveLength(2);
  });

  it("keeps display insertion in Design and source management in Data", () => {
    renderStudio();
    expect(screen.getAllByRole("button", { name: "Add to design" }).length)
      .toBeGreaterThan(0);

    fireEvent.click(modeRadios().data);
    expect(screen.queryByRole("button", { name: "Add to design" })).toBeNull();
    expect(screen.getByRole("button", { name: "Add source" }))
      .toBeInTheDocument();

    fireEvent.click(modeRadios().split);
    expect(screen.getAllByRole("button", { name: "Add to design" }).length)
      .toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Add source" }))
      .toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Hide data sources" }));
    expect(screen.getByRole("button", { name: "Show data sources" }))
      .toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Show data sources" }));
    expect(screen.getByRole("button", { name: "Hide data sources" }))
      .toBeInTheDocument();
  });

  it("keeps blank Split insertion in the Design canvas", async () => {
    renderStudio(createEmptyDraft("Blank"));
    fireEvent.click(
      screen.getByRole("radio", { name: "Split", exact: true }),
    );
    // The data pane remains an overview; the canvas owns insertion.
    expect(
      screen.getAllByRole("button", { name: "Add to design" }),
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

  it("uses variable IDs in studio diagnostics", async () => {
    previewMock.mockImplementation(async () => ({
      ok: false,
      draft_fingerprint: "fp-bad",
      diagnostics: [{ code: "variable_missing", variable_id: "month" }],
    }));
    renderStudio();

    await waitFor(() => {
      expect(screen.getByRole("alert", { name: "Diagnostics" }))
        .toHaveTextContent("Enter a value for month.");
    });
    expect(screen.getByRole("alert", { name: "Diagnostics" }))
      .not.toHaveTextContent("Month");
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
