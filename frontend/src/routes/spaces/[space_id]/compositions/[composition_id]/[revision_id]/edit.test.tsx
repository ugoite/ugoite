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

    // One Back to the opened revision, one Save, Design-first true-mode
    // structure with the single mode control: the finished shape only, no
    // Parameters, Tags, Data workspace, or Preview section.
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
    ).toEqual(["Design"]);
    expect(
      screen.queryByRole("heading", { name: "Display" }),
    ).not.toBeInTheDocument();

    // Data mode: the source row carries the Saved SQL entry name. Display
    // listing lives on the Design canvas now, not in a legacy section.
    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Studio mode" })).getByRole(
        "radio",
        { name: "Data" },
      ),
    );
    expect(screen.getAllByRole("button", { name: "Monthly totals" }))
      .toHaveLength(1);
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

  it("surfaces a stale base conflict without leaving the route", async () => {
    render(() => <CompositionEditRoute />);
    expect(await screen.findByLabelText("Name")).toHaveValue("Monthly review");

    // Another writer published first: the update carries the stale base
    // and the server rejects it instead of rewriting history.
    saveMock.mockRejectedValueOnce(
      new UgoiteApiError({
        kind: "conflict",
        operation: "composition.save",
        status: 409,
        code: "REVISION_CONFLICT",
        message: "base revision is stale",
      }),
    );
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
    // The rejected attempt stays on the route with the draft intact: no
    // new revision, no navigation, no silent retry.
    expect(navigateMock).not.toHaveBeenCalled();
    expect(screen.getByLabelText("Name")).toHaveValue("Monthly review");
    expect(
      screen.queryByRole("button", { name: "Retry" }),
    ).not.toBeInTheDocument();
  });

  it("reopens a restored multi-row revision for continued editing", async () => {
    // A restored revision carries text blocks and several rows; the edit
    // route loads it back into an editable draft so restore-then-edit
    // continues without flattening or rejecting known kinds.
    lintMock.mockResolvedValueOnce({
      ok: true,
      value: {
        document: {
          format: "ugoite.composition",
          format_version: 1,
          kind: "dashboard",
          name: "Monthly expenses",
          tags: [],
          spec: {
            parameters: [
              {
                id: "month_start",
                label: "Start month",
                type: "date",
                required: true,
                format: "year-month",
              },
            ],
            sources: [{
              kind: "saved_sql",
              id: "month_total",
              entry_id: "sql-1",
              revision_id: "sql-rev-1",
              expected_result: [{ name: "total", type: "float" }],
              variables: { month_start: { parameter: "month_start" } },
            }],
            components: [
              {
                kind: "text",
                id: "summary_title",
                text: "Monthly summary",
                style: "heading",
              },
              {
                kind: "metric",
                id: "total",
                label: "Monthly total",
                source: "month_total",
                value_field: { kind: "sql_column", name: "total" },
              },
            ],
            layout: {
              kind: "flow",
              rows: [
                {
                  id: "controls",
                  items: [{ kind: "parameter", parameter: "month_start" }],
                },
                {
                  id: "summary",
                  items: [
                    { kind: "component", component: "summary_title" },
                    { kind: "component", component: "total" },
                  ],
                },
              ],
            },
          },
        },
        canonical_yaml: storedYaml,
        fingerprint: "fingerprint",
      },
    });
    render(() => <CompositionEditRoute />);

    expect(await screen.findByLabelText("Name")).toHaveValue(
      "Monthly expenses",
    );
    expect(getMock).toHaveBeenCalledWith("space-1", "tool-1", "revision-1");
    // The studio opens in update mode: one Save, one way back, Design
    // first with the restored blocks on the canvas.
    expect(screen.getAllByRole("button", { name: "Save" })).toHaveLength(1);
    expect(screen.getByRole("link", { name: "Back to revision" }))
      .toHaveAttribute(
        "href",
        "/spaces/space-1/compositions/tool-1/revision-1",
      );
    // Data mode resolves the restored source to its Saved SQL entry name.
    fireEvent.click(
      within(screen.getByRole("radiogroup", { name: "Studio mode" })).getByRole(
        "radio",
        { name: "Data" },
      ),
    );
    expect(screen.getAllByRole("button", { name: "Monthly totals" }))
      .toHaveLength(1);
  });
});
