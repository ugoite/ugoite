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
  sqlGetMock,
  sqlQueryMock,
  navigateMock,
} = vi.hoisted(() => ({
  previewMock: vi.fn(),
  querySourceMock: vi.fn(),
  canonicalizeMock: vi.fn(),
  sqlGetMock: vi.fn(),
  sqlQueryMock: vi.fn(),
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
  compositionApi: {
    canonicalizeDocument: (...args: unknown[]) =>
      (canonicalizeMock as (...call: unknown[]) => unknown)(...args),
    preview: (...args: unknown[]) =>
      (previewMock as (...call: unknown[]) => unknown)(...args),
    querySource: (...args: unknown[]) =>
      (querySourceMock as (...call: unknown[]) => unknown)(...args),
  },
}));

vi.mock("~/lib/ugoite-client", () => ({
  sqlApi: {
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
