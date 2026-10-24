import "@testing-library/jest-dom/vitest";
import { createSignal, Show } from "solid-js";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { buildSqlSchema } from "~/lib/sql";
import type { SQLWorkspaceProps } from "./SQLWorkspace";
import { SQLWorkspace, SQLWorkspaceActionBar } from "./SQLWorkspace";
import { useDialogFocus } from "./dialog-focus";

vi.mock("./SqlQueryEditor", () => ({
  SqlQueryEditor: (props: {
    id?: string;
    value: string;
    ariaLabel?: string;
    disabled?: boolean;
    additionalDiagnostics?: readonly { message: string }[];
    onChange: (value: string) => void;
  }) => (
    <textarea
      id={props.id}
      aria-label={props.ariaLabel}
      data-planner-diagnostics={props.additionalDiagnostics?.map((item) =>
        item.message
      ).join("|") ?? ""}
      value={props.value}
      readOnly={props.disabled}
      onInput={(event) => props.onChange(event.currentTarget.value)}
    />
  ),
}));

const page = {
  columns: ["amount"],
  columnTypes: ["integer" as const],
  rows: [[42]],
  exactIntegerRows: [["42"]],
  pageIdentity: "sql-page-one",
  pageNumber: 1,
  canPrevious: false,
  canNext: true,
};

const defaultProps: SQLWorkspaceProps = {
  sql: 'SELECT "amount" FROM "G"',
  onSqlChange: () => undefined,
  schema: buildSqlSchema([]),
  onPreview: () => undefined,
  preview: { status: "idle", isCurrent: false },
};

const renderWorkspace = (overrides: Partial<SQLWorkspaceProps> = {}) =>
  render(() => <SQLWorkspace {...defaultProps} {...overrides} />);

describe("SQLWorkspace", () => {
  it("REQ-FE-080: keeps SQL editing controlled and passes the accessible name", () => {
    const onSqlChange = vi.fn();
    renderWorkspace({ sql: "SELECT 1", onSqlChange });

    const editor = screen.getByRole("textbox", { name: "SQL" });
    expect(editor).toHaveValue("SELECT 1");
    fireEvent.input(editor, { target: { value: "SELECT 2" } });
    expect(onSqlChange).toHaveBeenCalledWith("SELECT 2");
  });

  it("shows a current planner error as a local SQL diagnostic", () => {
    renderWorkspace({
      sql: "SELECT $missing",
      planner: {
        status: "error",
        isCurrent: true,
        error: "parameter $missing is not declared",
      },
    });

    expect(screen.getByRole("textbox", { name: "SQL" })).toHaveAttribute(
      "data-planner-diagnostics",
      "parameter $missing is not declared",
    );
  });

  it("does not show a stale planner error on the current SQL draft", () => {
    renderWorkspace({
      sql: "SELECT 2",
      planner: {
        status: "error",
        isCurrent: false,
        error: "old SQL failed planning",
      },
    });

    expect(screen.getByRole("textbox", { name: "SQL" })).toHaveAttribute(
      "data-planner-diagnostics",
      "",
    );
  });

  it("REQ-FE-080: keeps exact Saved SQL read-only while preview remains available", () => {
    const onPreview = vi.fn();
    renderWorkspace({
      readOnly: true,
      onPreview,
      preview: { status: "idle", isCurrent: false },
    });

    expect(screen.getByRole("textbox", { name: "SQL" })).toHaveAttribute(
      "readonly",
    );
    fireEvent.click(screen.getByRole("button", { name: "Run Query" }));
    expect(onPreview).toHaveBeenCalledOnce();
  });

  it("REQ-FE-080: lets the route disable preview while its source is read-only", () => {
    renderWorkspace({ previewDisabled: true });
    expect(screen.getByRole("button", { name: "Run Query" })).toBeDisabled();
  });

  it("REQ-FE-080: presents variable defaults and forwards raw transient values", () => {
    const onParameterChange = vi.fn();
    renderWorkspace({
      variables: [
        { name: "enabled", type: "boolean", default: true },
        { name: "threshold", type: "integer", default: 0 },
      ],
      parameterValues: { enabled: undefined },
      onParameterChange,
    });

    const enabled = screen.getByLabelText("enabled");
    expect(enabled).toHaveValue("");
    expect(screen.getByLabelText("threshold")).toHaveValue("0");
    fireEvent.input(enabled, { target: { value: "false" } });
    expect(onParameterChange).toHaveBeenCalledWith("enabled", "false");
  });

  it("REQ-FE-080: omits the variable pane when the query declares no variables", () => {
    renderWorkspace({ variables: [] });
    expect(screen.queryByRole("region", { name: "Variables" })).toBeNull();
  });

  it("REQ-FE-080: keeps editor and variable IDs unique across workspaces", () => {
    const { container } = render(() => (
      <>
        <SQLWorkspace
          {...defaultProps}
          variables={[{ name: "first", type: "string" }]}
        />
        <SQLWorkspace
          {...defaultProps}
          variables={[{ name: "second", type: "string" }]}
        />
      </>
    ));

    const editorIds = Array.from(
      container.querySelectorAll(".sql-workspace__editor textarea"),
      (editor) => editor.id,
    );
    const variableIds = Array.from(
      container.querySelectorAll(".sql-workspace__variable input"),
      (input) => input.id,
    );
    expect(new Set(editorIds).size).toBe(2);
    expect(new Set(variableIds).size).toBe(2);
  });

  it("REQ-FE-080: exposes operation reasons and never rewrites SQL on action", () => {
    const onSqlChange = vi.fn();
    const onColumns = vi.fn();
    renderWorkspace({
      onSqlChange,
      actions: [
        {
          kind: "columns",
          icon: "columns",
          label: "Columns",
          onActivate: onColumns,
        },
        {
          kind: "filter",
          icon: "filter",
          label: "Filter",
          disabled: true,
          disabledReason: "Unsupported SQL structure",
          onActivate: vi.fn(),
        },
      ],
    });

    const columns = screen.getByRole("button", { name: "Columns" });
    expect(columns).toHaveClass("icononly");
    // Touch Safari may leave focus elsewhere when a button is activated.
    expect(columns).not.toHaveFocus();
    fireEvent.click(columns);
    expect(onColumns).toHaveBeenCalledWith(columns);
    expect(onSqlChange).not.toHaveBeenCalled();

    const filter = screen.getByRole("button", {
      name: "Filter: Unsupported SQL structure",
    });
    expect(filter).toBeDisabled();
    expect(filter).toHaveAttribute(
      "title",
      "Filter: Unsupported SQL structure",
    );
  });

  it("renders one external action bar before the editor and results when requested", () => {
    const onActivate = vi.fn();
    const actions = [{
      kind: "form" as const,
      icon: "forms" as const,
      label: "Form",
      onActivate,
    }];
    const { container } = render(() => (
      <>
        <SQLWorkspaceActionBar actions={actions} />
        <SQLWorkspace
          {...defaultProps}
          actions={actions}
          hideActions
          preview={{ status: "success", isCurrent: true, page }}
        />
      </>
    ));

    const group = screen.getByRole("group", { name: "SQL" });
    const editor = screen.getByRole("textbox", { name: "SQL" });
    const results = container.querySelector(".sql-workspace__results");
    expect(results).not.toBeNull();
    expect(
      group.compareDocumentPosition(editor) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(
      group.compareDocumentPosition(results!) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(container.querySelectorAll(".sql-workspace__actions")).toHaveLength(
      1,
    );

    fireEvent.click(screen.getByRole("button", { name: "Form" }));
    expect(onActivate).toHaveBeenCalledOnce();
  });

  it("groups structure operations behind one of the six shared actions", async () => {
    const onJoin = vi.fn();
    const onAggregate = vi.fn();
    const onGroupBy = vi.fn();
    const onDistinct = vi.fn();
    const onRowLimit = vi.fn();
    const actions: SQLWorkspaceProps["actions"] = [
      { kind: "form", icon: "forms", label: "Form", onActivate: vi.fn() },
      {
        kind: "columns",
        icon: "columns",
        label: "Columns",
        onActivate: vi.fn(),
      },
      {
        kind: "filter",
        icon: "filter",
        label: "Filter",
        onActivate: vi.fn(),
      },
      { kind: "sort", icon: "sort", label: "Sort", onActivate: vi.fn() },
      {
        kind: "structure",
        icon: "group-by",
        label: "Structure",
        disabled: true,
        disabledReason: "Join unavailable",
        onActivate: onJoin,
      },
      {
        kind: "aggregate",
        icon: "aggregate",
        label: "Aggregate",
        onActivate: onAggregate,
      },
      {
        kind: "group_by",
        icon: "group-by",
        label: "Group by",
        onActivate: onGroupBy,
      },
      {
        kind: "distinct",
        icon: "distinct",
        label: "Distinct",
        onActivate: onDistinct,
      },
      {
        kind: "row_limit",
        icon: "row-limit",
        label: "Row limit",
        onActivate: onRowLimit,
      },
      {
        kind: "arguments",
        icon: "sql",
        label: "Arguments",
        onActivate: vi.fn(),
      },
    ];
    render(() => <SQLWorkspaceActionBar actions={actions} />);

    const primaryGroup = screen.getByRole("group", { name: "SQL" });
    expect(within(primaryGroup).getAllByRole("button")).toHaveLength(6);
    const structure = within(primaryGroup).getByRole("button", {
      name: "Structure",
    });
    expect(structure).toBeEnabled();
    fireEvent.click(structure);
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Structure" }),
      ).toHaveAttribute("aria-expanded", "true")
    );

    const structureGroup = screen.getByRole("group", {
      name: "Structure options",
    });
    expect(within(structureGroup).getAllByRole("button")).toHaveLength(5);
    expect(
      within(structureGroup).getByRole("button", {
        name: "Structure: Join unavailable",
      }),
    ).toBeDisabled();
    fireEvent.click(
      within(structureGroup).getByRole("button", { name: "Aggregate" }),
    );
    expect(onAggregate).toHaveBeenCalledOnce();
    expect(screen.queryByRole("group", { name: "Structure options" }))
      .toBeNull();
    expect(onJoin).not.toHaveBeenCalled();
    expect(onGroupBy).not.toHaveBeenCalled();
    expect(onDistinct).not.toHaveBeenCalled();
    fireEvent.click(
      within(primaryGroup).getByRole("button", {
        name: "Structure",
      }),
    );
    fireEvent.click(
      within(screen.getByRole("group", { name: "Structure options" }))
        .getByRole("button", { name: "Group by" }),
    );
    expect(onGroupBy).toHaveBeenCalledOnce();
    expect(screen.queryByRole("group", { name: "Structure options" }))
      .toBeNull();
    fireEvent.click(
      within(primaryGroup).getByRole("button", {
        name: "Structure",
      }),
    );
    fireEvent.click(
      within(screen.getByRole("group", { name: "Structure options" }))
        .getByRole("button", { name: "Row limit" }),
    );
    expect(onRowLimit).toHaveBeenCalledOnce();
  });

  it("keeps Structure reachable when every operation is locally unavailable", async () => {
    const unavailable = (
      kind: "structure" | "aggregate" | "group_by" | "distinct" | "row_limit",
      label: string,
    ) => ({
      kind,
      icon: "appearance" as const,
      label,
      disabled: true,
      disabledReason: "Unsupported by this SQL shape",
      onActivate: vi.fn(),
    });
    render(() => (
      <SQLWorkspaceActionBar
        actions={[
          unavailable("structure", "Join"),
          unavailable("aggregate", "Aggregate"),
          unavailable("group_by", "Group by"),
          unavailable("distinct", "Distinct"),
          unavailable("row_limit", "Row limit"),
        ]}
      />
    ));

    const structure = screen.getByRole("button", { name: "Structure" });
    expect(structure).toBeEnabled();
    fireEvent.click(structure);
    expect(
      await screen.findByRole("group", { name: "Structure options" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "Join: Unsupported by this SQL shape",
      }),
    ).toBeDisabled();
  });

  it("returns focus to Structure when a nested action opens and closes a dialog", async () => {
    const [dialogOpen, setDialogOpen] = createSignal(false);
    let dialog: HTMLDivElement | undefined;
    let returnFocus: HTMLElement | null = null;
    const Dialog = () => {
      useDialogFocus(() => dialogOpen(), {
        dialog: () => dialog,
        returnFocus: () => returnFocus,
        inert: false,
        onClose: () => setDialogOpen(false),
      });
      return (
        <Show when={dialogOpen()}>
          <div
            ref={(element) => dialog = element}
            role="dialog"
            aria-label="Row limit"
          >
            <button type="button" onClick={() => setDialogOpen(false)}>
              Close
            </button>
          </div>
        </Show>
      );
    };
    render(() => (
      <>
        <SQLWorkspaceActionBar
          actions={[
            {
              kind: "row_limit",
              icon: "row-limit",
              label: "Row limit",
              onActivate: (target) => {
                returnFocus = target ?? null;
                setDialogOpen(true);
              },
            },
          ]}
        />
        <Dialog />
      </>
    ));

    const structure = screen.getByRole("button", { name: "Structure" });
    fireEvent.click(structure);
    fireEvent.click(
      within(screen.getByRole("group", { name: "Structure options" }))
        .getByRole("button", { name: "Row limit" }),
    );
    expect(returnFocus).toBe(structure);
    const rowLimitDialog = await screen.findByRole("dialog", {
      name: "Row limit",
    });
    fireEvent.click(
      within(rowLimitDialog).getByRole("button", { name: "Close" }),
    );

    await waitFor(() => expect(structure).toHaveFocus());
  });

  it("REQ-FE-080: reports loading and errors without presenting a stale page", () => {
    const loading = renderWorkspace({
      preview: { status: "loading", isCurrent: false, page },
    });
    expect(screen.getByRole("status")).toHaveTextContent(
      "Loading SQL results...",
    );
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.getByRole("button", { name: "Run Query" })).toBeDisabled();
    loading.unmount();

    renderWorkspace({
      preview: {
        status: "error",
        isCurrent: false,
        error: "Access denied",
        page,
      },
    });
    expect(screen.getByRole("alert")).toHaveTextContent("Access denied");
    expect(screen.queryByRole("table")).toBeNull();
  });

  it("REQ-FE-080: keeps count explicit and delegates result paging", () => {
    const onCount = vi.fn();
    const onNext = vi.fn();
    renderWorkspace({
      preview: {
        status: "success",
        isCurrent: true,
        page,
        count: { status: "idle" },
      },
      onCount,
      onNext,
    });

    expect(screen.getByRole("table", { name: "SQL results" }))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Previous" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(onNext).toHaveBeenCalledOnce();
    expect(onCount).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Count rows" }));
    expect(onCount).toHaveBeenCalledOnce();
  });

  it("REQ-FE-080: presents explicit total count and an empty result state", () => {
    const { unmount } = renderWorkspace({
      preview: {
        status: "success",
        isCurrent: true,
        page,
        count: { status: "success", value: 25 },
      },
    });
    expect(screen.getByText("25 rows")).toBeInTheDocument();
    unmount();

    renderWorkspace({
      preview: {
        status: "success",
        isCurrent: true,
        page: { ...page, rows: [] },
      },
    });
    expect(screen.getByText("The query returned no rows.")).toBeInTheDocument();
  });

  it("REQ-FE-080: uses 16px inputs and 44px action targets", () => {
    const stylesheet = readFileSync(join(__dirname, "..", "app.css"), "utf8");
    expect(stylesheet).toMatch(
      /\.sql-workspace \.ui-sql-editor,[\s\S]*?font-size:\s*16px/,
    );
    expect(stylesheet).toMatch(
      /\.sql-workspace__variable \.ui-input\s*\{[^}]*font-size:\s*16px/s,
    );
    expect(stylesheet).not.toMatch(
      /\.sql-workspace__editor\s*\{[^}]*overflow:\s*hidden/s,
    );
    expect(stylesheet).toMatch(
      /\.sql-workspace__preview-action \.btn\s*\{[^}]*min-height:\s*44px/s,
    );
    expect(stylesheet).toMatch(
      /\.sql-workspace__result-controls \.btn\s*\{[^}]*min-height:\s*44px/s,
    );
    expect(stylesheet).toMatch(
      /\.icononly\s*\{[^}]*min-width:\s*44px/s,
    );
  });

  it("REQ-FE-080: hides a successful result page after its inputs change", () => {
    renderWorkspace({
      preview: { status: "success", isCurrent: false, page },
    });

    expect(screen.queryByRole("table", { name: "SQL results" })).toBeNull();
    expect(screen.queryByRole("region", { name: "Results" })).toBeNull();
  });
});
