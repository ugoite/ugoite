import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import CompositionNewRoute from "./new";

const {
  canonicalizeMock,
  saveMock,
  previewMock,
  querySourceMock,
  formListMock,
  sqlListMock,
  sqlGetMock,
  sqlQueryMock,
  navigateMock,
} = vi.hoisted(() => ({
  canonicalizeMock: vi.fn(),
  saveMock: vi.fn(),
  previewMock: vi.fn(),
  querySourceMock: vi.fn(),
  formListMock: vi.fn(),
  sqlListMock: vi.fn(),
  sqlGetMock: vi.fn(),
  sqlQueryMock: vi.fn(),
  navigateMock: vi.fn(),
}));

const locationControls = vi.hoisted(() => ({
  state: undefined as unknown,
}));

vi.mock("@solidjs/router", () => ({
  useParams: () => ({ space_id: "space-1" }),
  useLocation: () => ({
    pathname: "/spaces/space-1/compositions/new",
    get state() {
      return locationControls.state;
    },
  }),
  useNavigate: () => navigateMock,
  A: (props: {
    href: string;
    class?: string;
    children: unknown;
    ["aria-label"]?: string;
    title?: string;
  }) => (
    <a
      href={props.href}
      class={props.class}
      aria-label={props["aria-label"]}
      title={props.title}
    >
      {props.children as never}
    </a>
  ),
}));

vi.mock("~/lib/composition-api", () => ({
  compositionApi: {
    canonicalizeDocument: (...args: unknown[]) =>
      (canonicalizeMock as (...call: unknown[]) => unknown)(...args),
    save: (...args: unknown[]) =>
      (saveMock as (...call: unknown[]) => unknown)(...args),
    preview: (...args: unknown[]) =>
      (previewMock as (...call: unknown[]) => unknown)(...args),
    querySource: (...args: unknown[]) =>
      (querySourceMock as (...call: unknown[]) => unknown)(...args),
  },
  canCreateSavedSqlComposition: (entry: { kind: string }) =>
    entry.kind === "user-query",
  compositionDisplayName: (value: unknown) =>
    typeof value === "string" && value.trim() ? value.trim() : "Composition",
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

const FORM_ID = "11111111-1111-4111-8111-111111111111";

const taskForm = {
  id: FORM_ID,
  name: "Tasks",
  version: 1,
  template: "task",
  fields: {},
};

const monthlyEntry = {
  id: "sql-1",
  name: "Monthly",
  kind: "user-query",
  sql: "SELECT total FROM monthly",
  variables: [],
  created_at: "2026-10-01T00:00:00Z",
  updated_at: "2026-10-02T00:00:00Z",
  revision_id: "sql-rev-1",
};

const canonicalResult = {
  document: {},
  canonical_yaml: "canonical yaml",
  fingerprint: "fingerprint",
};

const emptyPlan = {
  ok: true,
  draft_fingerprint: "draft-fingerprint",
  parameter_definitions: [],
  plan: { draft_fingerprint: "draft-fingerprint", sources: [] },
};

describe("Composition studio shell", () => {
  // The Data section renders in Data mode; tests touching sources switch
  // the single mode control first.
  const showDataMode = () => {
    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Studio mode" })).getByRole(
        "radio",
        { name: "Data" },
      ),
    );
  };

  const showDesignMode = () => {
    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Studio mode" })).getByRole(
        "radio",
        { name: "Design" },
      ),
    );
  };

  const addSourceViaPicker = async (
    name: string,
    tab: "Forms" | "Saved SQL" = "Forms",
  ) => {
    const wasDataMode = screen.getByRole("radio", { name: "Data" })
      .getAttribute("aria-checked") === "true";
    showDesignMode();
    fireEvent.click(
      screen.getAllByRole("button", { name: "Add to design" })[0],
    );
    fireEvent.click(
      screen.getByRole("dialog", { name: "Add to design" }).querySelector(
        ".designPaletteItem:nth-child(2)",
      )!,
    );
    const dataPicker = await screen.findByRole("dialog", {
      name: "Add data component",
    });
    fireEvent.click(
      within(dataPicker).getByRole("button", {
        name: "Choose a Form or Saved SQL",
      }),
    );
    await screen.findByRole("dialog", { name: "Choose a source" });
    if (tab !== "Forms") {
      fireEvent.click(await screen.findByRole("tab", { name: tab }));
    }
    fireEvent.click(await screen.findByRole("button", { name }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    if (wasDataMode) showDataMode();
  };

  const addMetricViaCanvas = async (
    sourceName: string,
    label?: string,
  ) => {
    fireEvent.click(
      screen.getAllByRole("button", { name: "Add to design" })[0],
    );
    const palette = screen.getByRole("dialog", { name: "Add to design" });
    fireEvent.click(within(palette).getByRole("button", { name: "Data" }));
    const picker = await screen.findByRole("dialog", {
      name: "Add data component",
    });
    fireEvent.click(within(picker).getByRole("tab", { name: "Metric" }));
    fireEvent.click(within(picker).getByRole("button", { name: sourceName }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    if (label) {
      fireEvent.input(screen.getByLabelText("Label"), {
        target: { value: label },
      });
    }
  };

  beforeEach(() => {
    setLocale("en");
    vi.clearAllMocks();
    locationControls.state = undefined;
    canonicalizeMock.mockResolvedValue(canonicalResult);
    previewMock.mockResolvedValue(emptyPlan);
    formListMock.mockResolvedValue([taskForm]);
    sqlListMock.mockResolvedValue([monthlyEntry]);
    sqlGetMock.mockResolvedValue(monthlyEntry);
    sqlQueryMock.mockResolvedValue({
      columns: ["total"],
      rows: [[1]],
      has_more: false,
      result_schema: [{ name: "total", type: "float" }],
    });
  });

  afterEach(() => cleanup());

  it("renders one back control, one save, and mode-switched studio structure", async () => {
    const { container } = render(() => <CompositionNewRoute />);

    expect(screen.getAllByRole("link")).toHaveLength(1);
    expect(screen.getByRole("link", { name: "Saved tools" })).toHaveAttribute(
      "href",
      "/spaces/space-1/compositions",
    );
    // Save starts blocked on the missing name and carries the reason in its
    // accessible name and title instead of a prose paragraph.
    const saveButton = screen.getByRole("button", {
      name: "Save, Enter a name to save.",
    });
    expect(saveButton).toBeDisabled();
    expect(saveButton).toHaveAttribute(
      "title",
      "Save, Enter a name to save.",
    );

    // True modes: a blank Design shows the finished shape only — header,
    // mode switch, Design canvas area, inspector slot. Parameters, Tags,
    // the Data workspace, and the Preview section never render here. The
    // legacy Display section is gone; the canvas owns display editing.
    const modes = screen.getByRole("radiogroup", { name: "Studio mode" });
    const headings = screen.getAllByRole("heading", { level: 2 }).map(
      (heading) => heading.textContent,
    );
    expect(headings).toEqual(["Design"]);
    expect(
      screen.queryByRole("heading", { name: "Display" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Parameters" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Tags" })).not
      .toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Preview" })).not
      .toBeInTheDocument();

    // The tool-name input owns the name: typing updates the heading owner.
    const nameInput = screen.getByLabelText("Name");
    expect(nameInput).toHaveValue("");
    fireEvent.input(nameInput, { target: { value: "Weekly review" } });
    await waitFor(() => {
      expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
        "Weekly review",
      );
    });
    // With a name but no sources the reason follows the draft.
    expect(
      screen.getByRole("button", { name: "Save, Add data to save." }),
    ).toBeDisabled();

    // The blank canvas opens the ordinary insertion palette without prose.
    expect(container.querySelectorAll("p")).toHaveLength(0);
    expect(
      screen.getAllByRole("button", { name: "Add to design" }),
    ).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Add to design" }));
    fireEvent.click(
      screen.getByRole("dialog", { name: "Add to design" }).querySelector(
        ".designPaletteItem:nth-child(2)",
      )!,
    );
    const dataPicker = await screen.findByRole("dialog", {
      name: "Add data component",
    });
    expect(
      within(dataPicker).getByRole("button", {
        name: "Choose a Form or Saved SQL",
      }),
    ).toBeInTheDocument();
    fireEvent.click(
      within(dataPicker).getByRole("button", {
        name: "Choose a Form or Saved SQL",
      }),
    );
    expect(await screen.findByRole("dialog", { name: "Choose a source" }))
      .toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });

    // Data mode lists existing sources without a competing source action.
    // Parameters disclose progressively only once a source exists; Tags
    // remain on their own tab.
    fireEvent.click(within(modes).getByRole("radio", { name: "Data" }));
    expect(
      screen.getAllByRole("heading", { level: 2 }).map((heading) =>
        heading.textContent
      ),
    ).toEqual(["Data"]);
    expect(screen.getByRole("tab", { name: "Sources" }))
      .toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Tags" })).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Parameters" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Select Tasks" }),
    ).not.toBeInTheDocument();
    expect(container.querySelectorAll("p")).toHaveLength(0);
    expect(screen.queryByRole("button", { name: "Add to design" }))
      .not.toBeInTheDocument();
  });

  it("discloses Parameters in Data mode only once a source exists", async () => {
    render(() => <CompositionNewRoute />);
    showDataMode();

    expect(
      screen.queryByRole("heading", { name: "Parameters" }),
    ).not.toBeInTheDocument();
    await addSourceViaPicker("Tasks");

    expect(screen.getByRole("tab", { name: "Parameters" }))
      .toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Parameters" }))
      .not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Parameters" }));
    expect(screen.getByRole("heading", { name: "Parameters" }))
      .toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Tags" })).not
      .toBeInTheDocument();
    // Data mode never renders the canvas or the removed Preview section.
    expect(screen.queryByRole("heading", { name: "Preview" })).not
      .toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Select Tasks" }),
    ).not.toBeInTheDocument();
  });

  it("adds data sources with full-row selection and keyboard-operable reorder", async () => {
    const { container } = render(() => <CompositionNewRoute />);
    showDataMode();

    await addSourceViaPicker("Tasks");
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    showDataMode();

    expect(screen.getByRole("button", { name: "Tasks" }))
      .toBeInTheDocument();
    const removeTasks = screen.getByRole("button", { name: "Remove Tasks" });
    expect(removeTasks).toHaveAttribute("aria-label", "Remove Tasks");
    expect(
      screen.getByRole("button", { name: "Move Tasks up" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Move Tasks down" }),
    ).toBeDisabled();

    // The selected source keeps its human-readable heading without exposing
    // its internal Form ID in a nested disclosure.
    expect(screen.getByRole("heading", { name: "Tasks" }))
      .toBeInTheDocument();
    expect(container.querySelector("details")).toBeNull();
    expect(container.textContent).not.toContain(FORM_ID);
    fireEvent.click(screen.getByRole("button", { name: "Tasks" }));
    expect(container.querySelector(".dataWorkspaceEditor")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Tasks" }));
    expect(container.querySelector(".dataWorkspaceEditor")).not.toBeNull();
    expect(container.textContent).not.toContain(FORM_ID);

    await addSourceViaPicker("Monthly", "Saved SQL");
    showDataMode();
    expect(sqlGetMock).toHaveBeenCalledWith("space-1", "sql-1");

    const order = () =>
      Array.from(container.querySelectorAll(".rowListItem")).map((item) =>
        item.textContent
      );
    expect(order()[0]).toMatch(/Tasks/);
    fireEvent.click(screen.getByRole("button", { name: "Move Monthly up" }));
    expect(order()[0]).toMatch(/Monthly/);

    // Source removal is blocked while its table still uses it. Remove that
    // design block first, then remove the unreferenced source in Data.
    showDesignMode();
    fireEvent.click(screen.getByRole("button", { name: "Select Tasks" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove Tasks" }));
    showDataMode();
    fireEvent.click(screen.getByRole("button", { name: "Remove Tasks" }));
    expect(
      screen.queryByRole("button", { name: "Tasks" }),
    ).not.toBeInTheDocument();
  });

  it("adds canvas blocks with label, reorder, and remove through canvas and inspector", async () => {
    const { container } = render(() => <CompositionNewRoute />);
    fireEvent.input(screen.getByLabelText("Name"), {
      target: { value: "Weekly review" },
    });
    showDataMode();
    await addSourceViaPicker("Tasks");
    showDesignMode();

    // A newly chosen source immediately provides its first table; the canvas
    // selects it, and the inspector owns its optional label.
    expect(
      screen.getByRole("button", { name: "Select Tasks" }),
    ).toBeInTheDocument();
    fireEvent.input(screen.getByLabelText("Label"), {
      target: { value: "Details" },
    });
    expect(
      screen.getByRole("button", { name: "Select Details" }),
    ).toBeInTheDocument();

    showDataMode();
    await addSourceViaPicker("Monthly", "Saved SQL");
    showDesignMode();
    await addMetricViaCanvas("Monthly", "Total");
    expect(
      screen.getByRole("button", { name: "Select Total" }),
    ).toBeInTheDocument();

    // Row reorder through the canvas; adding the metric opens its own row.
    const rowOrder = () =>
      Array.from(container.querySelectorAll(".designRow")).map((row) =>
        row.getAttribute("data-row-id")
      );
    const beforeReorder = rowOrder();
    expect(beforeReorder).toHaveLength(3);
    fireEvent.click(screen.getByRole("button", { name: "Move row 1 down" }));
    expect(rowOrder()).toEqual([
      beforeReorder[1],
      beforeReorder[0],
      beforeReorder[2],
    ]);

    // Remove through the canvas clears the block and blocks saving again.
    // Removing the selected metric clears the selection, so the table
    // block needs selecting before its own remove renders.
    fireEvent.click(screen.getByRole("button", { name: "Remove Total" }));
    expect(
      screen.queryByRole("button", { name: "Select Total" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Select Details" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove Details" }));
    expect(
      screen.queryByRole("button", { name: "Select Details" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Select Monthly" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove Monthly" }));
    expect(
      screen.queryByRole("button", { name: "Select Monthly" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "Save, Add a block to the canvas to save.",
      }),
    ).toBeDisabled();
    expect(saveMock).not.toHaveBeenCalled();
  });

  it("saves the canonical draft with a stable idempotency key", async () => {
    render(() => <CompositionNewRoute />);

    fireEvent.input(screen.getByLabelText("Name"), {
      target: { value: "Weekly review" },
    });
    showDataMode();
    await addSourceViaPicker("Tasks");
    showDesignMode();

    const saveButton = screen.getByRole("button", { name: "Save" });
    expect(saveButton).toBeEnabled();

    saveMock.mockResolvedValueOnce({
      composition_id: "tool-1",
      revision_id: "revision-2",
      canonical_yaml: "canonical yaml",
      receipt: {
        command_id: "command-1",
        catalog_generation: 4,
        snapshot_id: 42,
        committed_revision_ids: ["revision-2"],
        committed_at_micros: 1,
        data_file_count: 1,
      },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(saveMock).toHaveBeenCalledTimes(1);
    });
    expect(saveMock).toHaveBeenCalledWith(
      "space-1",
      "canonical yaml",
      expect.any(String),
    );
    expect(navigateMock).toHaveBeenCalledWith(
      "/spaces/space-1/compositions/tool-1/revision-2",
    );
  });

  it("retries an uncertain save with the identical payload and key", async () => {
    render(() => <CompositionNewRoute />);

    fireEvent.input(screen.getByLabelText("Name"), {
      target: { value: "Weekly review" },
    });
    showDataMode();
    await addSourceViaPicker("Tasks");
    showDesignMode();

    saveMock.mockRejectedValueOnce(new Error("transport closed"));
    saveMock.mockResolvedValueOnce({
      composition_id: "tool-1",
      revision_id: "revision-2",
      canonical_yaml: "canonical yaml",
      receipt: {
        command_id: "command-1",
        catalog_generation: 4,
        snapshot_id: 42,
        committed_revision_ids: ["revision-2"],
        committed_at_micros: 1,
        data_file_count: 1,
      },
    });

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(saveMock).toHaveBeenCalledTimes(1);
    });
    expect(await screen.findByText("Could not save this tool."))
      .toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => {
      expect(saveMock).toHaveBeenCalledTimes(2);
    });
    // The retry reuses the identical payload and idempotency key.
    expect(saveMock.mock.calls[1]).toEqual(saveMock.mock.calls[0]);
    expect(navigateMock).toHaveBeenCalledWith(
      "/spaces/space-1/compositions/tool-1/revision-2",
    );
  });

  it("adds a source and its initial table together from Design", async () => {
    render(() => <CompositionNewRoute />);

    fireEvent.input(screen.getByLabelText("Name"), {
      target: { value: "Weekly review" },
    });
    showDataMode();
    await addSourceViaPicker("Tasks");
    showDesignMode();

    expect(screen.getByRole("button", { name: "Select Tasks" }))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    expect(canonicalizeMock).not.toHaveBeenCalled();
    expect(saveMock).not.toHaveBeenCalled();
  });

  it("surfaces the allowlisted diagnostic when canonicalization rejects the draft", async () => {
    render(() => <CompositionNewRoute />);

    fireEvent.input(screen.getByLabelText("Name"), {
      target: { value: "Weekly review" },
    });
    showDataMode();
    await addSourceViaPicker("Tasks");
    showDesignMode();

    canonicalizeMock.mockRejectedValueOnce(
      new UgoiteApiError({
        kind: "composition_diagnostic",
        message: "invalid composition",
        code: "invalid_composition",
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("The tool definition is invalid."))
      .toBeInTheDocument();
    expect(saveMock).not.toHaveBeenCalled();
  });

  it("falls back to the generic failure when canonicalization transport fails", async () => {
    render(() => <CompositionNewRoute />);

    fireEvent.input(screen.getByLabelText("Name"), {
      target: { value: "Weekly review" },
    });
    showDataMode();
    await addSourceViaPicker("Tasks");
    showDesignMode();

    canonicalizeMock.mockRejectedValueOnce(new Error("transport closed"));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Could not save this tool."))
      .toBeInTheDocument();
    expect(saveMock).not.toHaveBeenCalled();
  });

  it("seeds the studio from a saved sql seed with a table and placed controls", () => {
    locationControls.state = {
      seed: {
        kind: "saved_sql",
        seed: {
          entryId: "sql-1",
          revisionId: "sql-rev-1",
          name: "Monthly",
          expectedResult: [{ name: "total", type: "float" }],
          variables: { month: { parameter: "month" } },
          variableTypes: { month: "date" },
        },
      },
    };
    const replaceState = vi.spyOn(window.history, "replaceState");
    render(() => <CompositionNewRoute />);
    showDataMode();

    expect(screen.getByRole("button", { name: "Monthly" }))
      .toBeInTheDocument();
    expect(screen.getByLabelText("Name")).toHaveValue("Monthly");
    expect(screen.getByText("month")).toBeInTheDocument();
    // The seed carries a default Table on the new source with the variable
    // control placed, so the canvas owns a visible block on open.
    showDesignMode();
    expect(
      screen.getByRole("button", { name: "Select Monthly" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    // The seed is consumed once: the location state is cleared on mount so
    // back/forward never double-adds the source.
    expect(replaceState).toHaveBeenCalledWith(
      null,
      "",
      "/spaces/space-1/compositions/new",
    );
    replaceState.mockRestore();
  });

  it("seeds the studio from an entry query seed with a default table", () => {
    locationControls.state = {
      seed: {
        kind: "entry_query",
        seed: {
          formId: FORM_ID,
          name: "Tasks",
          fieldSchema: [],
          query: {
            filters: [],
            sort: [],
            projection: { kind: "preview" },
          },
        },
      },
    };
    render(() => <CompositionNewRoute />);
    showDataMode();

    expect(screen.getByRole("button", { name: "Tasks" })).toBeInTheDocument();
    expect(screen.getByLabelText("Name")).toHaveValue("Tasks");
    // The seed carries a default Table, so the Studio opens save-ready
    // with a visible block instead of a zero-display draft.
    showDesignMode();
    expect(
      screen.getByRole("button", { name: "Select Tasks" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("provisions typed parameters from saved sql variables and guards removal", async () => {
    sqlGetMock.mockResolvedValue({
      ...monthlyEntry,
      variables: [{ name: "month", type: "date", description: "" }],
    });
    render(() => <CompositionNewRoute />);
    showDataMode();
    await addSourceViaPicker("Monthly", "Saved SQL");

    // The month parameter is provisioned from the server-declared type.
    // Open its Data tab. Scoped to the Parameters section: the source viewer also
    // surfaces the Saved SQL variable name in its own Variables section.
    fireEvent.click(screen.getByRole("tab", { name: "Parameters" }));
    const parameters = screen
      .getByRole("heading", { name: "Parameters" })
      .closest("section")!;
    expect(await within(parameters).findByText("month")).toBeInTheDocument();

    // A referenced parameter cannot be removed silently.
    fireEvent.click(screen.getByRole("button", { name: "Remove month" }));
    expect(
      await screen.findByText("This parameter is used by a data source."),
    ).toBeInTheDocument();
    expect(within(parameters).getByText("month")).toBeInTheDocument();
  });
});
