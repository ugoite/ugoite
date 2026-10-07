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

  const addSourceViaPicker = async (name: string) => {
    fireEvent.click(screen.getByRole("button", { name: "Add data" }));
    fireEvent.click(await screen.findByRole("button", { name }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
  };

  // Canvas insertion path: a gap "+" opens the block palette, and the
  // palette metric/table entries delegate to the display picker at the
  // recorded target. The legacy Display section is gone; this is the only
  // insertion path.
  const addTableViaCanvas = async (sourceName: string) => {
    fireEvent.click(screen.getAllByRole("button", { name: "Add block" })[0]);
    const palette = screen.getByRole("dialog", { name: "Add block" });
    fireEvent.click(within(palette).getByRole("button", { name: "Table" }));
    const picker = await screen.findByRole("dialog", { name: "Add display" });
    fireEvent.click(within(picker).getByRole("button", { name: "Table" }));
    fireEvent.click(within(picker).getByRole("button", { name: sourceName }));
    fireEvent.click(
      within(picker).getByRole("button", { name: "Add display" }),
    );
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
  };

  const addMetricViaCanvas = async (
    sourceName: string,
    value: string,
    label: string,
  ) => {
    fireEvent.click(screen.getAllByRole("button", { name: "Add block" })[0]);
    const palette = screen.getByRole("dialog", { name: "Add block" });
    fireEvent.click(within(palette).getByRole("button", { name: "Metric" }));
    const picker = await screen.findByRole("dialog", { name: "Add display" });
    fireEvent.click(within(picker).getByRole("button", { name: "Metric" }));
    fireEvent.click(within(picker).getByRole("button", { name: sourceName }));
    fireEvent.change(within(picker).getByLabelText("Value"), {
      target: { value },
    });
    fireEvent.input(within(picker).getByLabelText("Label"), {
      target: { value: label },
    });
    fireEvent.click(
      within(picker).getByRole("button", { name: "Add display" }),
    );
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
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

    // The blank canvas guides with one structural Add-data action and no
    // prose paragraphs; it opens the single source picker dialog.
    expect(container.querySelectorAll("p")).toHaveLength(0);
    expect(
      screen.getAllByRole("button", { name: "Add data" }),
    ).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Add data" }));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });

    // Data mode brings the fetch definition only: Data workspace, then
    // Tags. Parameters disclose progressively only once a source exists,
    // and the canvas and Preview section never render here.
    fireEvent.click(within(modes).getByRole("radio", { name: "Data" }));
    expect(
      screen.getAllByRole("heading", { level: 2 }).map((heading) =>
        heading.textContent
      ),
    ).toEqual(["Data", "Tags"]);
    expect(
      screen.queryByRole("heading", { name: "Parameters" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Select Tasks" }),
    ).not.toBeInTheDocument();
    const paragraphs = container.querySelectorAll("p");
    expect(paragraphs).toHaveLength(1);
    expect(paragraphs[0]).toHaveTextContent("Add data to begin.");
  });

  it("discloses Parameters in Data mode only once a source exists", async () => {
    render(() => <CompositionNewRoute />);
    showDataMode();

    expect(
      screen.queryByRole("heading", { name: "Parameters" }),
    ).not.toBeInTheDocument();
    await addSourceViaPicker("Tasks");

    expect(screen.getByRole("heading", { name: "Parameters" }))
      .toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Tags" })).toBeInTheDocument();
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

    fireEvent.click(screen.getByRole("button", { name: "Add data" }));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    fireEvent.click(await screen.findByRole("button", { name: "Tasks" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });

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

    // Full-row activation toggles the advanced revision disclosure. The
    // newly added source starts expanded; activating its row collapses it.
    expect(container.querySelector("details")).not.toBeNull();
    expect(
      within(container.querySelector("details") as HTMLElement).getByText(
        `Form ${FORM_ID}`,
      ),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Tasks" }));
    expect(container.querySelector("details")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Tasks" }));
    expect(container.querySelector("details")).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Add data" }));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    fireEvent.click(await screen.findByRole("button", { name: "Monthly" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect(sqlGetMock).toHaveBeenCalledWith("space-1", "sql-1");

    const order = () =>
      Array.from(container.querySelectorAll(".rowListItem")).map((item) =>
        item.textContent
      );
    expect(order()[0]).toMatch(/Tasks/);
    fireEvent.click(screen.getByRole("button", { name: "Move Monthly up" }));
    expect(order()[0]).toMatch(/Monthly/);

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
    // A source-only draft cannot save: the disabled Save carries the
    // layout reason instead of a prose paragraph.
    expect(
      screen.getByRole("button", {
        name: "Save, Add a block to the canvas to save.",
      }),
    ).toBeDisabled();
    showDesignMode();

    await addTableViaCanvas("Tasks");
    // The new block is selected: the canvas owns the block, the inspector
    // owns its label.
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
    await addSourceViaPicker("Monthly");
    showDesignMode();
    await addMetricViaCanvas("Monthly", "total", "Total");
    expect(
      screen.getByRole("button", { name: "Select Total" }),
    ).toBeInTheDocument();

    // Row reorder through the canvas; each insertion opened its own row.
    const rowOrder = () =>
      Array.from(container.querySelectorAll(".designRow")).map((row) =>
        row.getAttribute("data-row-id")
      );
    expect(rowOrder()).toEqual(["row-2", "row-1"]);
    fireEvent.click(screen.getByRole("button", { name: "Move row 1 down" }));
    expect(rowOrder()).toEqual(["row-1", "row-2"]);

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
    await addTableViaCanvas("Tasks");

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
    await addTableViaCanvas("Tasks");

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

  it("blocks a source-only save with an accessible reason and no save call", async () => {
    render(() => <CompositionNewRoute />);

    fireEvent.input(screen.getByLabelText("Name"), {
      target: { value: "Weekly review" },
    });
    showDataMode();
    await addSourceViaPicker("Tasks");

    // The gate blocks the attempt before canonicalization: the disabled
    // Save carries the layout reason, and no save call is ever made.
    const saveButton = screen.getByRole("button", {
      name: "Save, Add a block to the canvas to save.",
    });
    expect(saveButton).toBeDisabled();
    expect(saveButton).toHaveAttribute(
      "title",
      "Save, Add a block to the canvas to save.",
    );
    fireEvent.click(saveButton);
    await waitFor(() => {
      expect(canonicalizeMock).not.toHaveBeenCalled();
    });
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
    await addTableViaCanvas("Tasks");

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
    await addTableViaCanvas("Tasks");

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

    fireEvent.click(screen.getByRole("button", { name: "Add data" }));
    fireEvent.click(await screen.findByRole("button", { name: "Monthly" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });

    // The month parameter is provisioned from the server-declared type.
    // Scoped to the Parameters section: the Data workspace viewer also
    // surfaces the Saved SQL variable name in its own Variables section.
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
