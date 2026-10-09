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

/** Controllable viewport for the Studio matchMedia gates. */
type MediaListener = (event: { matches: boolean; media: string }) => void;
let viewportWidth = 1280;
const mediaRegistries: Array<{
  query: string;
  listeners: Set<MediaListener>;
}> = [];

const evaluateQuery = (query: string): boolean => {
  const match = query.match(/\(max-width:\s*(\d+)px\)/);
  return match ? viewportWidth <= Number(match[1]) : false;
};

const stubViewportMedia = () => {
  mediaRegistries.length = 0;
  vi.stubGlobal("matchMedia", (query: string) => {
    const listeners = new Set<MediaListener>();
    mediaRegistries.push({ query, listeners });
    return {
      get matches() {
        return evaluateQuery(query);
      },
      media: query,
      addEventListener: (_type: string, listener: MediaListener) => {
        listeners.add(listener);
      },
      removeEventListener: (_type: string, listener: MediaListener) => {
        listeners.delete(listener);
      },
      addListener: (listener: MediaListener) => {
        listeners.add(listener);
      },
      removeListener: (listener: MediaListener) => {
        listeners.delete(listener);
      },
    };
  });
};

const setViewportWidth = (width: number) => {
  viewportWidth = width;
  for (const entry of mediaRegistries) {
    const event = { matches: evaluateQuery(entry.query), media: entry.query };
    for (const listener of [...entry.listeners]) listener(event);
  }
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

describe("CompositionStudioMobile", () => {
  beforeEach(() => {
    setLocale("en");
    vi.clearAllMocks();
    viewportWidth = 1280;
    stubViewportMedia();
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

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("opens the selected block inspector in a bottom sheet on narrow viewports and returns focus on close", async () => {
    setViewportWidth(390);
    const { container } = renderStudio();

    fireEvent.click(screen.getByRole("button", { name: "Select Total" }));

    // The inspector renders once, inside the sheet: the dialog carries the
    // selected block name while the inline canvas slot stays empty.
    const dialog = await screen.findByRole("dialog", { name: "Metric" });
    expect(dialog).toHaveClass("studioInspectorSheet");
    expect(container.querySelector(".studioDesign aside")).toBeNull();
    expect(
      within(dialog).getByLabelText("Label"),
    ).toBeInTheDocument();
    // Focus moves into the sheet on open: the Close control leads.
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Close" })).toHaveFocus();
    });

    // Escape dismisses and returns focus to the invoking block; dismissal
    // clears the transient selection so reopening stays a single tap.
    // Focus return waits a microtask so it never races the sheet lifting
    // `inert` in its own disposal.
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Select Total" }),
      ).toHaveFocus();
    });
    expect(
      container.querySelector(".designBlock[data-selected]"),
    ).toBeNull();

    // Backdrop dismiss follows the same path.
    fireEvent.click(screen.getByRole("button", { name: "Select Total" }));
    const reopened = await screen.findByRole("dialog", { name: "Metric" });
    fireEvent.click(reopened.parentElement!);
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Select Total" }),
      ).toHaveFocus();
    });
  });

  it("keeps the inspector inline with no dialog on wide viewports", () => {
    setViewportWidth(1280);
    const { container } = renderStudio();

    fireEvent.click(screen.getByRole("button", { name: "Select Total" }));

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(
      container.querySelector(".studioDesign aside"),
    ).not.toBeNull();
    expect(
      screen.getByRole("heading", { name: "Metric" }),
    ).toBeInTheDocument();
    expect(
      container.querySelector(
        '[data-block-id="disp-1"] .designBlockActions [aria-label^="Remove "]',
      ),
    ).toBeNull();
    expect(
      container.querySelector(".studioDesign aside")?.querySelector(
        '[aria-label="Remove Total"]',
      ),
    ).not.toBeNull();
  });

  it("removes a selected display from the narrow Inspector sheet and returns focus to the canvas", async () => {
    setViewportWidth(390);
    const { container } = renderStudio();

    fireEvent.click(screen.getByRole("button", { name: "Select Total" }));
    const dialog = await screen.findByRole("dialog", { name: "Metric" });
    expect(
      within(dialog).getByRole("button", { name: "Remove Total" }),
    ).toBeEnabled();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Remove Total" }),
    );

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Select Total" }),
    ).toBeNull();
    await waitFor(() => {
      expect(
        container.querySelector(".designCanvas [data-block-id] button"),
      ).toHaveFocus();
    });

    // Removing a display leaves its source available in the Data workspace.
    fireEvent.click(modeRadios().data);
    await screen.findByRole("heading", { name: "Monthly totals" });
  });

  it("unplaces a parameter from the narrow Inspector sheet but keeps its declaration", async () => {
    setViewportWidth(390);
    renderStudio();

    fireEvent.click(screen.getByRole("button", { name: "Select Month" }));
    const dialog = await screen.findByRole("dialog", { name: "Month" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Unplace Month" }),
    );

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Select Month" }),
    ).toBeNull();
    fireEvent.click(modeRadios().data);
    fireEvent.click(screen.getByRole("tab", { name: "Parameters" }));
    expect(
      screen.getByRole("button", { name: "Remove Month" }),
    ).toBeInTheDocument();
  });

  it("forces Design when crossing below the Split gate", () => {
    setViewportWidth(1280);
    const { container } = renderStudio();
    const modes = modeRadios();

    fireEvent.click(modes.split);
    expect(modes.split).toHaveAttribute("aria-checked", "true");
    expect(container.querySelector(".studioSplit")).not.toBeNull();

    // Resizing into a narrow viewport never keeps a stacked Split: the
    // switch returns to Design, offering Design | Data only.
    setViewportWidth(700);
    expect(modes.design).toHaveAttribute("aria-checked", "true");
    expect(container.querySelector(".studioSplit")).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(
      screen.getByRole("button", { name: "Select Total" }),
    ).toBeInTheDocument();
  });

  it("moves the sheet data jump to the Data workspace on narrow viewports", async () => {
    setViewportWidth(390);
    renderStudio();

    fireEvent.click(screen.getByRole("button", { name: "Select Total" }));
    const dialog = await screen.findByRole("dialog", { name: "Metric" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Data" }));

    // The sheet dismisses and the jumped source renders in Data.
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(modeRadios().data).toHaveAttribute("aria-checked", "true");
    await screen.findByRole("heading", { name: "Monthly totals" });
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Monthly totals" }),
      ).toHaveFocus();
    });
  });

  it("opens the sheet for a freshly inserted block on narrow viewports", async () => {
    setViewportWidth(390);
    renderStudio();

    // The palette opens the direct Table/Metric source picker; the fresh
    // block owns the inspector on add.
    fireEvent.click(
      screen.getAllByRole("button", {
        name: "Add to design",
        exact: true,
      })[0],
    );
    const palette = await screen.findByRole("dialog", {
      name: "Add to design",
      exact: true,
    });
    fireEvent.click(
      within(palette).getByRole("button", { name: "Data", exact: true }),
    );
    const picker = await screen.findByRole("dialog", {
      name: "Add data component",
      exact: true,
    });
    const kindGroup = within(picker).getByRole("tablist", {
      name: "Data component type",
    });
    expect(
      within(kindGroup).getByRole("tab", { name: "Table" }),
    ).toHaveAttribute("aria-selected", "true");
    expect(
      within(kindGroup).getByRole("tab", { name: "Metric" }),
    ).toHaveAttribute("aria-selected", "false");
    fireEvent.click(
      within(picker).getByRole("button", { name: "Monthly totals" }),
    );

    await screen.findByRole("dialog", { name: "Table" });
    expect(screen.queryByRole("dialog", { name: "Add data component" }))
      .toBeNull();
  });

  it("keeps the mode switch single-row and the sheet within the viewport at 320px and 390px widths", () => {
    const css = stylesheet();
    // The canvas stacks to one column at the sheet gate, so 390px and the
    // 320px effective width never scroll the document sideways.
    expect(css).toMatch(
      /@media\s*\(max-width:\s*560px\)[\s\S]*?\.studioDesign\s*\{\s*grid-template-columns:\s*minmax\(0,\s*1fr\)/,
    );
    // The mode switch keeps one operative row at narrow widths: no wrap
    // rule, with tightened density below the sheet gate for 320px.
    expect(css).toMatch(/\.studioMode\s*\{[^}]*display:\s*inline-flex/);
    expect(css).not.toMatch(/\.studioMode\s*\{[^}]*flex-wrap:\s*wrap/);
    expect(css).toMatch(
      /@media\s*\(max-width:\s*560px\)[\s\S]*?\.studioMode\s*\{/,
    );
    // The bottom sheet bounds itself to the viewport with internal scroll.
    expect(css).toMatch(
      /\.ui-dialog\.studioInspectorSheet\s*\{[^}]*width:\s*min\(560px,\s*100%\)/,
    );
    expect(css).toMatch(
      /\.ui-dialog\.studioInspectorSheet\s*\{[^}]*overflow-y:\s*auto/,
    );
    expect(css).toMatch(
      /\.ui-backdrop\.studioSheetBackdrop\s*\{[^}]*align-items:\s*end/,
    );
    expect(css).toMatch(
      /\.studioInspectorTitleBar\s*\{[^}]*justify-content:\s*space-between/,
    );
    expect(css).toMatch(
      /\.studioInspectorTitleBar \.studioInspectorTitle\s*\{[^}]*min-width:\s*0/,
    );
  });
});
