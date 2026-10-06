import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import CompositionEditRoute from "./edit";

const {
  getMock,
  lintMock,
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
  getMock: vi.fn(),
  lintMock: vi.fn(),
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

vi.mock("@solidjs/router", () => ({
  useParams: () => ({
    space_id: "space-1",
    composition_id: "tool-1",
    revision_id: "revision-1",
  }),
  useLocation: () => ({
    pathname: "/spaces/space-1/compositions/tool-1/revision-1/edit",
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
    get: (...args: unknown[]) =>
      (getMock as (...call: unknown[]) => unknown)(...args),
    lint: (...args: unknown[]) =>
      (lintMock as (...call: unknown[]) => unknown)(...args),
    canonicalizeDocument: (...args: unknown[]) =>
      (canonicalizeMock as (...call: unknown[]) => unknown)(...args),
    save: (...args: unknown[]) =>
      (saveMock as (...call: unknown[]) => unknown)(...args),
    preview: (...args: unknown[]) =>
      (previewMock as (...call: unknown[]) => unknown)(...args),
    querySource: (...args: unknown[]) =>
      (querySourceMock as (...call: unknown[]) => unknown)(...args),
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

const storedYaml = "format: ugoite.composition\nname: Monthly review\n";

const lintDocument = {
  format: "ugoite.composition",
  format_version: 1,
  kind: "dashboard",
  name: "Monthly review",
  tags: ["finance"],
  spec: {
    parameters: [],
    sources: [{
      kind: "saved_sql",
      id: "src-1",
      entry_id: "sql-1",
      revision_id: "sql-rev-1",
      expected_result: [{ name: "total", type: "float" }],
      variables: {},
    }],
    components: [{ kind: "table", id: "disp-1", source: "src-1" }],
    layout: {
      kind: "flow",
      rows: [{
        id: "main",
        items: [{ kind: "component", component: "disp-1" }],
      }],
    },
  },
};

const savedEntry = {
  id: "sql-1",
  name: "Monthly totals",
  kind: "user-query",
  sql: "SELECT total FROM monthly",
  variables: [],
  created_at: "2026-10-01T00:00:00Z",
  updated_at: "2026-10-02T00:00:00Z",
  revision_id: "sql-rev-1",
};

const savedResponse = {
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
};

describe("Composition edit route", () => {
  beforeEach(() => {
    setLocale("en");
    vi.clearAllMocks();
    getMock.mockResolvedValue({
      revision: {
        entry_id: "tool-1",
        revision_id: "revision-1",
        committed_at_micros: 1,
      },
      fields: { spec: storedYaml },
      unmapped_field_values: {},
    });
    lintMock.mockResolvedValue({
      ok: true,
      value: {
        document: lintDocument,
        canonical_yaml: storedYaml,
        fingerprint: "fingerprint",
      },
    });
    canonicalizeMock.mockResolvedValue({
      document: {},
      canonical_yaml: "canonical yaml",
      fingerprint: "fingerprint",
    });
    previewMock.mockResolvedValue({
      ok: true,
      draft_fingerprint: "draft-fingerprint",
      parameter_definitions: [],
      plan: { draft_fingerprint: "draft-fingerprint", sources: [] },
    });
    formListMock.mockResolvedValue([]);
    sqlListMock.mockResolvedValue([savedEntry]);
    sqlGetMock.mockResolvedValue(savedEntry);
    sqlQueryMock.mockResolvedValue({
      columns: ["total"],
      rows: [[1]],
      has_more: false,
      result_schema: [{ name: "total", type: "float" }],
    });
  });

  afterEach(() => cleanup());

  it("loads the exact revision through get and lint with a prefilled studio", async () => {
    render(() => <CompositionEditRoute />);

    expect(await screen.findByLabelText("Name")).toHaveValue("Monthly review");
    expect(getMock).toHaveBeenCalledWith("space-1", "tool-1", "revision-1");
    expect(lintMock).toHaveBeenCalledWith(storedYaml);

    // One Back to the opened revision, one Save, data-first structure.
    expect(screen.getAllByRole("link")).toHaveLength(1);
    expect(screen.getByRole("link", { name: "Back to revision" }))
      .toHaveAttribute(
        "href",
        "/spaces/space-1/compositions/tool-1/revision-1",
      );
    expect(screen.getAllByRole("button", { name: "Save" })).toHaveLength(1);
    expect(
      screen.getAllByRole("heading", { level: 2 }).map((heading) =>
        heading.textContent
      ),
    ).toEqual(["Data", "Display", "Parameters", "Tags", "Preview"]);

    // The source and display rows carry the Saved SQL entry name.
    expect(screen.getAllByRole("button", { name: "Monthly totals" }))
      .toHaveLength(2);
  });

  it("saves an update with base revision identity and a stable retry key", async () => {
    render(() => <CompositionEditRoute />);
    expect(await screen.findByLabelText("Name")).toHaveValue("Monthly review");

    saveMock.mockRejectedValueOnce(new Error("transport closed"));
    saveMock.mockResolvedValueOnce(savedResponse);

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(saveMock).toHaveBeenCalledTimes(1);
    });
    expect(saveMock).toHaveBeenCalledWith(
      "space-1",
      "canonical yaml",
      expect.any(String),
      { compositionId: "tool-1", baseRevisionId: "revision-1" },
    );
    expect(await screen.findByText("Could not save this tool."))
      .toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => {
      expect(saveMock).toHaveBeenCalledTimes(2);
    });
    // The retry reuses the identical payload, identity, and key.
    expect(saveMock.mock.calls[1]).toEqual(saveMock.mock.calls[0]);
    expect(navigateMock).toHaveBeenCalledWith(
      "/spaces/space-1/compositions/tool-1/revision-2",
    );
  });

  it("refuses an invalid lint without opening the studio", async () => {
    lintMock.mockResolvedValueOnce({
      ok: false,
      error: { kind: "composition_diagnostic", code: "invalid_composition" },
    });
    render(() => <CompositionEditRoute />);

    expect(await screen.findByText("This revision cannot be edited."))
      .toBeInTheDocument();
    expect(getMock).toHaveBeenCalledWith("space-1", "tool-1", "revision-1");
    // The studio never opens: no name input, no Save, one way back.
    expect(screen.queryByLabelText("Name")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save" })).not
      .toBeInTheDocument();
    expect(screen.getAllByRole("link")).toHaveLength(1);
    expect(screen.getByRole("link", { name: "Back to revision" }))
      .toHaveAttribute(
        "href",
        "/spaces/space-1/compositions/tool-1/revision-1",
      );
    expect(saveMock).not.toHaveBeenCalled();
  });
});
