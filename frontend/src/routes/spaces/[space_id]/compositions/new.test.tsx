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
    expect(screen.getAllByRole("button", { name: "Save" })).toHaveLength(1);

    // One segmented mode control: Design first, Data on demand.
    const modes = screen.getByRole("radiogroup", { name: "Studio mode" });
    const headings = screen.getAllByRole("heading", { level: 2 }).map(
      (heading) => heading.textContent,
    );
    expect(headings).toEqual([
      "Design",
      "Display",
      "Parameters",
      "Tags",
      "Preview",
    ]);

    // The tool-name input owns the name: typing updates the heading owner.
    const nameInput = screen.getByLabelText("Name");
    expect(nameInput).toHaveValue("");
    fireEvent.input(nameInput, { target: { value: "Weekly review" } });
    await waitFor(() => {
      expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
        "Weekly review",
      );
    });

    // Design-mode empty states carry one next-action line each, nothing else.
    expect(container.querySelectorAll("p")).toHaveLength(2);

    // Data mode brings the Data section first with its own empty state.
    fireEvent.click(within(modes).getByRole("radio", { name: "Data" }));
    expect(
      screen.getAllByRole("heading", { level: 2 }).map((heading) =>
        heading.textContent
      ),
    ).toEqual(["Data", "Display", "Parameters", "Tags", "Preview"]);
    const paragraphs = container.querySelectorAll("p");
    expect(paragraphs).toHaveLength(3);
    expect(paragraphs[0]).toHaveTextContent("Add data to begin.");
    expect(paragraphs[1]).toHaveTextContent("Add a display to begin.");
    expect(paragraphs[2]).toHaveTextContent("Add a parameter to begin.");
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

  it("adds displays between data and tags with headers in order", async () => {
    const { container } = render(() => <CompositionNewRoute />);
    showDataMode();

    const headings = () =>
      screen.getAllByRole("heading", { level: 2 }).map((heading) =>
        heading.textContent
      );
    expect(headings()).toEqual([
      "Data",
      "Display",
      "Parameters",
      "Tags",
      "Preview",
    ]);

    // Displays need a source first.
    expect(screen.getByRole("button", { name: "Add display" })).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Add data" }));
    fireEvent.click(await screen.findByRole("button", { name: "Tasks" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: "Add display" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Add display" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Table" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Tasks" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add display" }),
    );
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });

    // The display row owns its default source name; headers stay ordered.
    expect(headings()).toEqual([
      "Data",
      "Display",
      "Parameters",
      "Tags",
      "Preview",
    ]);
    expect(
      within(screen.getByRole("region", { name: "Display" })).getByText(
        "Table",
      ),
    ).toBeInTheDocument();
    expect(
      within(container).getAllByRole("button", { name: "Tasks" }),
    ).toHaveLength(2);

    fireEvent.click(
      screen.getAllByRole("button", { name: "Remove Tasks" })[1],
    );
    expect(
      within(container).getAllByRole("button", { name: "Tasks" }),
    ).toHaveLength(1);
  });

  it("saves the canonical draft with a stable idempotency key", async () => {
    render(() => <CompositionNewRoute />);
    showDataMode();

    const saveButton = screen.getByRole("button", { name: "Save" });
    expect(saveButton).toBeDisabled();

    fireEvent.input(screen.getByLabelText("Name"), {
      target: { value: "Weekly review" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add data" }));
    fireEvent.click(await screen.findByRole("button", { name: "Tasks" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });

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
    showDataMode();

    fireEvent.input(screen.getByLabelText("Name"), {
      target: { value: "Weekly review" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add data" }));
    fireEvent.click(await screen.findByRole("button", { name: "Tasks" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });

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

  it("seeds the studio from a saved sql seed with a prefilled name", () => {
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

  it("seeds the studio from an entry query seed with a prefilled name", () => {
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
