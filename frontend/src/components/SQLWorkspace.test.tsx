import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { buildSqlSchema } from "~/lib/sql";
import type { SQLWorkspaceProps } from "./SQLWorkspace";
import { SQLWorkspace } from "./SQLWorkspace";

vi.mock("./SqlQueryEditor", () => ({
  SqlQueryEditor: (props: {
    id?: string;
    value: string;
    ariaLabel?: string;
    disabled?: boolean;
    onChange: (value: string) => void;
  }) => (
    <textarea
      id={props.id}
      aria-label={props.ariaLabel}
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
    fireEvent.click(columns);
    expect(onColumns).toHaveBeenCalledOnce();
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
