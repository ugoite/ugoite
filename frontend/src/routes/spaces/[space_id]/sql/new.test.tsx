import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SpaceQueryCreateRoute from "./new";
import { expectBackLinkAtHeaderStart } from "~/test/back-link-placement";

const {
  navigateMock,
  formApiListMock,
  sqlCreateMock,
  sqlQueryMock,
  sqlCountMock,
} = vi.hoisted(() => ({
  navigateMock: vi.fn(),
  formApiListMock: vi.fn(),
  sqlCreateMock: vi.fn(),
  sqlQueryMock: vi.fn(),
  sqlCountMock: vi.fn(),
}));

vi.mock("@solidjs/router", () => ({
  A: (props: {
    href: string;
    class?: string;
    children: unknown;
    "aria-label"?: string;
    title?: string;
  }) => (
    <a
      href={props.href}
      class={props.class}
      aria-label={props["aria-label"]}
      title={props.title}
    >
      {props.children}
    </a>
  ),
  useNavigate: () => navigateMock,
  useParams: () => ({ space_id: "default" }),
}));

vi.mock("~/components/SQLWorkspace", () => ({
  SQLWorkspace: (props: {
    id?: string;
    sql: string;
    onSqlChange?: (value: string) => void;
    onPreview: () => void;
    onDiagnostics?: (
      diagnostics: Array<{
        from: number;
        to: number;
        severity: "error";
        message: string;
      }>,
    ) => void;
    preview: { status: string };
  }) => {
    const emitBackendDiagnostic = (value: string) => {
      props.onDiagnostics?.(
        value.includes("missing_relation")
          ? [{
            from: 0,
            to: value.length,
            severity: "error",
            message: "Backend rejected the relation",
          }]
          : [],
      );
    };
    return (
      <div>
        <textarea
          id={props.id}
          aria-label="SQL"
          value={props.sql}
          onInput={(event) => {
            const value = event.currentTarget.value;
            props.onSqlChange?.(value);
            emitBackendDiagnostic(value);
          }}
        />
        <button type="button" onClick={props.onPreview}>Run Query</button>
        <output>{props.preview.status}</output>
      </div>
    );
  },
}));

vi.mock("~/components/SqlStructureToolbar", () => ({
  SqlStructureToolbar: (props: { onSqlChange?: (value: string) => void }) => (
    <button
      type="button"
      onClick={() => props.onSqlChange?.("SELECT 100")}
    >
      Apply SQL structure
    </button>
  ),
}));

vi.mock("~/lib/ugoite-client", () => ({
  formApi: { list: formApiListMock },
  sqlApi: {
    create: sqlCreateMock,
    query: sqlQueryMock,
    count: sqlCountMock,
  },
}));

describe("/spaces/:space_id/sql/new", () => {
  beforeEach(() => {
    navigateMock.mockReset();
    formApiListMock.mockResolvedValue([
      {
        id: "sql-form",
        name: "SQL",
        sql_relation: "form_sql",
        version: 1,
        template: "",
        fields: {},
      },
      {
        id: "user-form",
        name: "User",
        sql_relation: "form_user",
        version: 1,
        template: "",
        fields: {},
      },
      {
        id: "group-form",
        name: "UserGroup",
        sql_relation: "form_group",
        version: 1,
        template: "",
        fields: {},
      },
      {
        id: "form-1",
        name: "Entry",
        sql_relation: "form_entry",
        version: 1,
        template: "# Entry\n",
        fields: {},
      },
      {
        id: "review-form",
        name: "Review",
        sql_relation: "form_review",
        version: 1,
        template: "# Review\n",
        fields: {
          Amount: {
            id: 1,
            type: "integer",
            required: false,
            label: "Amount",
            sql_column: "column_amount",
            query_capability: {
              field: { kind: "property", field_id: 1 },
              name: "Amount",
              field_type: "integer",
              filterable: true,
              sortable: true,
              projectable: true,
              supported_operators: ["equals"],
            },
          },
          Category: {
            id: 2,
            type: "string",
            required: false,
            label: "Category",
            sql_column: "column_category",
            query_capability: {
              field: { kind: "property", field_id: 2 },
              name: "Category",
              field_type: "string",
              filterable: true,
              sortable: true,
              projectable: true,
              supported_operators: ["equals", "contains"],
            },
          },
          "00000000-0000-4000-8000-000000000003": {
            id: 3,
            type: "string",
            required: false,
            sql_column: "column_hidden",
            query_capability: {
              field: { kind: "property", field_id: 3 },
              name: "Field 3",
              field_type: "string",
              filterable: true,
              sortable: true,
              projectable: true,
              supported_operators: ["equals"],
            },
          },
        },
      },
    ]);
    sqlCreateMock.mockResolvedValue({ id: "query-1", revisionId: "rev-1" });
    sqlQueryMock.mockReset().mockResolvedValue({
      columns: ["value"],
      rows: [[1]],
      has_more: false,
      result_schema: [{ name: "value", type: "integer" }],
    });
    sqlCountMock.mockReset().mockResolvedValue(1);
  });

  it("previews the current draft through stateless SQL without creating Saved SQL", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => {
      expect(editor).toHaveValue(
        'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
      );
    });
    fireEvent.input(editor, { target: { value: "SELECT 1" } });
    fireEvent.click(screen.getByRole("button", { name: "Run Query" }));

    await waitFor(() => {
      expect(sqlQueryMock).toHaveBeenCalledWith("default", {
        sql: "SELECT 1",
        parameters: {},
        parameter_types: {},
        include_exact_integer_values: true,
        limit: 100,
      }, expect.any(AbortSignal));
    });
    expect(sqlCreateMock).not.toHaveBeenCalled();
    expect(await screen.findByText("success")).toBeInTheDocument();
  });

  it("previews generated Form SQL before replacing hand-edited SQL", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() =>
      expect(editor).toHaveValue(
        'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
      )
    );
    fireEvent.input(editor, { target: { value: "SELECT 99" } });

    fireEvent.click(
      screen.getByRole("button", { name: "Generate SQL from Form" }),
    );

    expect(editor).toHaveValue("SELECT 99");
    const proposal = screen.getByRole("region", {
      name: "Generated SQL preview",
    });
    expect(proposal.querySelector(".ui-text-danger")).toHaveTextContent(
      "− SELECT 99",
    );
    expect(proposal.querySelector(".ui-text-success")).toHaveTextContent(
      '+ SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
    );
    const screenReaderDiffLines = Array.from(
      proposal.querySelectorAll(".ui-sr-only"),
      (line) => line.textContent?.trim(),
    );
    expect(screenReaderDiffLines).toContain("Removed SQL line: SELECT 99");
    expect(screenReaderDiffLines).toContain(
      'Added SQL line: SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Use generated SQL" }),
    );
    expect(editor).toHaveValue(
      'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
    );
    expect(screen.queryByRole("region", {
      name: "Generated SQL preview",
    })).not.toBeInTheDocument();
  });

  it("reveals and applies captured generated SQL when the diff exceeds preview bounds", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() =>
      expect(editor).toHaveValue(
        'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
      )
    );
    const oversizedSql = "x".repeat(256 * 1024 + 1);
    const generatedSql =
      'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id';
    fireEvent.input(editor, { target: { value: oversizedSql } });

    fireEvent.click(
      screen.getByRole("button", { name: "Generate SQL from Form" }),
    );

    const proposal = screen.getByRole("region", {
      name: "Generated SQL preview",
    });
    expect(proposal).toHaveTextContent("SQL diff is too large to preview");
    const disclosure = proposal.querySelector("details");
    expect(disclosure).not.toHaveAttribute("open");

    fireEvent.click(screen.getByText("Show full generated SQL"));

    expect(disclosure).toHaveAttribute("open");
    expect(disclosure?.querySelector("pre")).toHaveTextContent(generatedSql);
    fireEvent.click(
      screen.getByRole("button", { name: "Use generated SQL" }),
    );
    expect(editor).toHaveValue(generatedSql);
  });

  it("keeps an aggregate SQL proposal tied to the edited query until applied", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() =>
      expect(editor).toHaveValue(
        'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
      )
    );
    fireEvent.input(editor, { target: { value: "SELECT 99" } });
    fireEvent.change(screen.getByLabelText("Form for this query"), {
      target: { value: "review-form" },
    });
    fireEvent.change(screen.getByLabelText("Aggregate"), {
      target: { value: "sum" },
    });
    fireEvent.change(screen.getByLabelText("Filter field"), {
      target: { value: "Category" },
    });

    fireEvent.click(
      screen.getByRole("button", { name: "Generate aggregate SQL" }),
    );

    expect(editor).toHaveValue("SELECT 99");
    expect(screen.getByRole("region", {
      name: "Generated SQL preview",
    })).toHaveTextContent(
      'SELECT SUM("column_amount") AS "value" FROM "Review" WHERE "column_category" = {{filter_value}}',
    );
    expect(
      screen.queryByRole("checkbox", { name: "Default for filter_value" }),
    ).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Use generated SQL" }),
    );
    expect(editor).toHaveValue(
      'SELECT SUM("column_amount") AS "value" FROM "Review" WHERE "column_category" = {{filter_value}}',
    );
    expect(
      await screen.findByRole("checkbox", { name: "Default for filter_value" }),
    ).toBeInTheDocument();
  });

  it("discards a generated SQL proposal when the editor changes", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() =>
      expect(editor).toHaveValue(
        'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
      )
    );
    fireEvent.input(editor, { target: { value: "SELECT 99" } });
    fireEvent.click(
      screen.getByRole("button", { name: "Generate SQL from Form" }),
    );
    expect(screen.getByRole("region", {
      name: "Generated SQL preview",
    })).toBeInTheDocument();

    fireEvent.input(editor, { target: { value: "SELECT 100" } });

    expect(editor).toHaveValue("SELECT 100");
    expect(screen.queryByRole("region", {
      name: "Generated SQL preview",
    })).not.toBeInTheDocument();
  });

  it("discards a generated SQL proposal when the structure toolbar edits SQL", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() =>
      expect(editor).toHaveValue(
        'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
      )
    );
    fireEvent.input(editor, { target: { value: "SELECT 99" } });
    fireEvent.click(
      screen.getByRole("button", { name: "Generate SQL from Form" }),
    );
    expect(screen.getByRole("region", {
      name: "Generated SQL preview",
    })).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "Apply SQL structure" }),
    );

    expect(editor).toHaveValue("SELECT 100");
    expect(screen.queryByRole("region", {
      name: "Generated SQL preview",
    })).not.toBeInTheDocument();
  });

  it("keeps the current query and variable metadata when a proposal is dismissed", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() =>
      expect(editor).toHaveValue(
        'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
      )
    );
    fireEvent.input(editor, { target: { value: "SELECT 99" } });
    fireEvent.change(screen.getByLabelText("Form for this query"), {
      target: { value: "review-form" },
    });
    fireEvent.change(screen.getByLabelText("Aggregate"), {
      target: { value: "sum" },
    });
    fireEvent.change(screen.getByLabelText("Field"), {
      target: { value: "Amount" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Generate aggregate SQL" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Keep current SQL" }),
    );

    expect(editor).toHaveValue("SELECT 99");
    expect(screen.queryByRole("region", {
      name: "Generated SQL preview",
    })).not.toBeInTheDocument();

    fireEvent.input(editor, {
      target: { value: "SELECT {{filter_value}}" },
    });

    expect(screen.getByText("string")).toBeInTheDocument();
    expect(screen.queryByText("integer")).not.toBeInTheDocument();
  });

  it("previews metadata-only changes before applying the generated parameter", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() =>
      expect(editor).toHaveValue(
        'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
      )
    );
    const sql =
      'SELECT SUM("column_amount") AS "value" FROM "Review" WHERE "column_amount" = {{filter_value}}';
    fireEvent.input(editor, { target: { value: sql } });
    fireEvent.change(screen.getByLabelText("Form for this query"), {
      target: { value: "review-form" },
    });
    fireEvent.change(screen.getByLabelText("Aggregate"), {
      target: { value: "sum" },
    });
    fireEvent.change(screen.getByLabelText("Filter field"), {
      target: { value: "Amount" },
    });

    fireEvent.click(
      screen.getByRole("button", { name: "Generate aggregate SQL" }),
    );

    expect(editor).toHaveValue(sql);
    const metadata = screen.getByText("filter_value").parentElement;
    expect(metadata).toHaveTextContent("string");
    expect(metadata).not.toHaveTextContent("integer");
    expect(screen.getByRole("region", {
      name: "Generated SQL preview",
    })).toHaveTextContent(
      "Variable filter_value: string → integer; description: None → Amount",
    );
    expect(screen.getByRole("region", {
      name: "Generated SQL preview",
    })).toHaveTextContent("SQL unchanged");

    fireEvent.click(
      screen.getByRole("button", { name: "Use generated SQL" }),
    );

    expect(editor).toHaveValue(sql);
    const appliedMetadata = screen.getByText("filter_value").parentElement;
    expect(appliedMetadata).toHaveTextContent("integer");
    expect(appliedMetadata).toHaveTextContent("Amount");
  });

  it("previews removing variable metadata before applying unfiltered SQL", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() =>
      expect(editor).toHaveValue(
        'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
      )
    );
    fireEvent.change(screen.getByLabelText("Form for this query"), {
      target: { value: "review-form" },
    });
    fireEvent.change(screen.getByLabelText("Aggregate"), {
      target: { value: "sum" },
    });
    fireEvent.change(screen.getByLabelText("Filter field"), {
      target: { value: "Amount" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Generate aggregate SQL" }),
    );

    const currentVariable = screen.getByText("filter_value").parentElement;
    expect(currentVariable).toHaveTextContent("integer");
    expect(currentVariable).toHaveTextContent("Amount");

    fireEvent.change(screen.getByLabelText("Filter field"), {
      target: { value: "" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Generate aggregate SQL" }),
    );

    expect(editor).toHaveValue(
      'SELECT SUM("column_amount") AS "value" FROM "Review" WHERE "column_amount" = {{filter_value}}',
    );
    const proposal = screen.getByRole("region", {
      name: "Generated SQL preview",
    });
    expect(proposal).toHaveTextContent(
      "Variable filter_value will be removed (integer; description: Amount)",
    );
    expect(proposal.querySelector(".ui-text-danger")).toHaveTextContent(
      '− SELECT SUM("column_amount") AS "value" FROM "Review" WHERE "column_amount" = {{filter_value}}',
    );
    expect(proposal.querySelector(".ui-text-success")).toHaveTextContent(
      '+ SELECT SUM("column_amount") AS "value" FROM "Review"',
    );
    expect(screen.getByText("filter_value").parentElement).toHaveTextContent(
      "integer",
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Use generated SQL" }),
    );

    expect(editor).toHaveValue(
      'SELECT SUM("column_amount") AS "value" FROM "Review"',
    );
    expect(screen.queryByText("filter_value")).not.toBeInTheDocument();
    expect(screen.queryByText("integer")).not.toBeInTheDocument();
  });

  it("discards a proposal when generator settings change", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() =>
      expect(editor).toHaveValue(
        'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
      )
    );
    fireEvent.input(editor, { target: { value: "SELECT 99" } });
    fireEvent.click(
      screen.getByRole("button", { name: "Generate SQL from Form" }),
    );
    expect(screen.getByRole("region", {
      name: "Generated SQL preview",
    })).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Form for this query"), {
      target: { value: "review-form" },
    });

    expect(editor).toHaveValue("SELECT 99");
    expect(screen.queryByRole("region", {
      name: "Generated SQL preview",
    })).not.toBeInTheDocument();
  });

  it("locks the reviewed SQL proposal while a save is in flight", async () => {
    let resolveCreate!: (value: { id: string; revisionId: string }) => void;
    sqlCreateMock.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveCreate = resolve;
      }),
    );
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() =>
      expect(editor).toHaveValue(
        'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
      )
    );
    fireEvent.input(editor, { target: { value: "SELECT 99" } });
    fireEvent.click(
      screen.getByRole("button", { name: "Generate SQL from Form" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    const apply = screen.getByRole("button", { name: "Use generated SQL" });
    const dismiss = screen.getByRole("button", { name: "Keep current SQL" });
    await waitFor(() => {
      expect(apply).toBeDisabled();
      expect(dismiss).toBeDisabled();
    });
    expect(editor).toHaveValue("SELECT 99");
    resolveCreate({ id: "query-1", revisionId: "rev-1" });
    await waitFor(() => expect(navigateMock).toHaveBeenCalled());
  });

  it("REQ-FE-063: starts with a Form relation and keeps paging out of SQL", async () => {
    render(() => <SpaceQueryCreateRoute />);

    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => {
      expect(editor).toHaveValue(
        'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
      );
    });
    fireEvent.change(screen.getByLabelText("Form for this query"), {
      target: { value: "review-form" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Generate SQL from Form" }),
    );
    await waitFor(() => {
      expect(editor).toHaveValue(
        'SELECT * FROM "Review" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
      );
    });
    fireEvent.input(screen.getByLabelText("Query name"), {
      target: { value: "Runnable starter" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(sqlCreateMock).toHaveBeenCalledWith("default", {
        name: "Runnable starter",
        kind: "user-query",
        metadata: undefined,
        sql:
          'SELECT * FROM "Review" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
        variables: [],
      });
    });
    expect(navigateMock).toHaveBeenCalledWith("/spaces/default/sql");
  });

  it("normalizes template variables to the native SQL query placeholder", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => {
      expect(editor).toHaveValue(
        'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
      );
    });
    fireEvent.input(editor, {
      target: {
        value:
          'SELECT * FROM "form_entry" WHERE Body = {{title}} ORDER BY _ugoite_id',
      },
    });
    fireEvent.input(screen.getByLabelText("Query name"), {
      target: { value: "Variable query" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(sqlCreateMock).toHaveBeenCalledWith("default", {
        name: "Variable query",
        kind: "user-query",
        metadata: undefined,
        sql:
          'SELECT * FROM "form_entry" WHERE Body = $title ORDER BY _ugoite_id',
        variables: [{ type: "string", name: "title", description: "" }],
      });
    });
  });

  it("saves a typed default for a generated integer filter variable", async () => {
    render(() => <SpaceQueryCreateRoute />);

    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() =>
      expect(editor).toHaveValue(
        'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
      )
    );
    fireEvent.change(screen.getByLabelText("Form for this query"), {
      target: { value: "review-form" },
    });
    fireEvent.change(screen.getByLabelText("Aggregate"), {
      target: { value: "sum" },
    });
    fireEvent.change(screen.getByLabelText("Filter field"), {
      target: { value: "Amount" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Generate aggregate SQL" }),
    );

    fireEvent.click(
      screen.getByRole("checkbox", { name: "Default for filter_value" }),
    );
    fireEvent.input(
      screen.getByRole("textbox", { name: "Default for filter_value" }),
      {
        target: { value: "12" },
      },
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(sqlCreateMock).toHaveBeenCalledWith(
        "default",
        expect.objectContaining({
          variables: [{
            type: "integer",
            name: "filter_value",
            description: "Amount",
            default: 12,
          }],
        }),
      );
    });
  });

  it("REQ-FE-063: builds a typed aggregate query from human Form fields", async () => {
    render(() => <SpaceQueryCreateRoute />);

    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => {
      expect(editor).toHaveValue(
        'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
      );
    });
    fireEvent.change(screen.getByLabelText("Form for this query"), {
      target: { value: "review-form" },
    });
    await waitFor(() => {
      expect(screen.getByLabelText("Form for this query")).toHaveValue(
        "review-form",
      );
    });
    fireEvent.change(screen.getByLabelText("Aggregate"), {
      target: { value: "sum" },
    });

    const aggregateField = screen.getByLabelText("Field") as HTMLSelectElement;
    await waitFor(() => {
      expect([...aggregateField.options].map((option) => option.textContent))
        .toEqual(["Amount"]);
    });
    const filterField = screen.getByLabelText(
      "Filter field",
    ) as HTMLSelectElement;
    expect([...filterField.options].map((option) => option.textContent))
      .toEqual(["None", "Amount", "Category"]);
    expect(screen.queryByRole("option", {
      name: "00000000-0000-4000-8000-000000000003",
    })).not.toBeInTheDocument();

    fireEvent.change(filterField, { target: { value: "Category" } });
    fireEvent.click(
      screen.getByRole("button", { name: "Generate aggregate SQL" }),
    );
    await waitFor(() => {
      expect(editor).toHaveValue(
        'SELECT SUM("column_amount") AS "value" FROM "Review" WHERE "column_category" = {{filter_value}}',
      );
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(sqlCreateMock).toHaveBeenCalledWith("default", {
        name: null,
        kind: "user-query",
        metadata: { generatedName: "untitled" },
        sql:
          'SELECT SUM("column_amount") AS "value" FROM "Review" WHERE "column_category" = $filter_value',
        variables: [{
          type: "string",
          name: "filter_value",
          description: "Category",
        }],
      });
    });
  });

  it("leaves session semantic validation to the backend", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => {
      expect(editor).toHaveValue(
        'SELECT * FROM "Entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id',
      );
    });
    fireEvent.input(editor, {
      target: { value: "SELECT * FROM missing_relation LIMIT $page_size" },
    });
    expect(await screen.findByText(/Backend rejected the relation/))
      .toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(sqlCreateMock).toHaveBeenCalledWith("default", {
        name: null,
        kind: "user-query",
        metadata: { generatedName: "untitled" },
        sql: "SELECT * FROM missing_relation LIMIT $page_size",
        variables: [{ type: "string", name: "page_size", description: "" }],
      });
    });
  });

  it("PR6: backs to Saved SQL once and reports diagnostics with line and column", async () => {
    render(() => <SpaceQueryCreateRoute />);

    const back = await screen.findByRole("link", { name: "Back to Saved SQL" });
    expect(back).toHaveAttribute("href", "/spaces/default/sql");
    expect(screen.getAllByRole("link", { name: "Back to Saved SQL" }))
      .toHaveLength(1);
    expectBackLinkAtHeaderStart(back);

    const editor = await screen.findByRole("textbox", { name: "SQL" });
    fireEvent.input(editor, {
      target: { value: "SELECT * FROM missing_relation LIMIT $page_size" },
    });
    expect(await screen.findByText(/Line 1, column \d+: /))
      .toBeInTheDocument();
  });
});
