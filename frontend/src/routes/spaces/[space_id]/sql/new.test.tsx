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
  structureActionMock,
} = vi.hoisted(() => ({
  navigateMock: vi.fn(),
  formApiListMock: vi.fn(),
  sqlCreateMock: vi.fn(),
  sqlQueryMock: vi.fn(),
  sqlCountMock: vi.fn(),
  structureActionMock: vi.fn(),
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

vi.mock("~/components/SQLWorkspace", async (importOriginal) => {
  const actual = await importOriginal<
    typeof import("~/components/SQLWorkspace")
  >();
  return {
    ...actual,
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
      hideActions?: boolean;
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
        <div
          data-testid="sql-workspace"
          data-hide-actions={props.hideActions ? "true" : "false"}
        >
          <section class="sql-workspace__editor" aria-label="SQL">
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
          </section>
          <button type="button" onClick={props.onPreview}>Run Query</button>
          <output>{props.preview.status}</output>
          {props.preview.status === "success" && (
            <section
              class="sql-workspace__results"
              aria-label="SQL results"
            >
              Rows
            </section>
          )}
        </div>
      );
    },
  };
});

vi.mock("~/components/SqlStructureToolbar", async () => {
  const { onMount } = await import("solid-js");
  return {
    SqlStructureToolbar: (props: {
      onActionsChange: (
        actions: readonly {
          kind: "form";
          icon: "forms";
          label: string;
          onActivate: () => void;
        }[],
      ) => void;
    }) => {
      onMount(() =>
        props.onActionsChange([{
          kind: "form",
          icon: "forms",
          label: "Form",
          onActivate: structureActionMock,
        }])
      );
      return null;
    },
  };
});

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
    structureActionMock.mockReset();
  });

  it("previews the current draft through stateless SQL without creating Saved SQL", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => expect(editor).toHaveValue("SELECT 1"));
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

    const actionGroup = screen.getByRole("group", { name: "SQL" });
    const formAction = screen.getByRole("button", { name: "Form" });
    const results = screen.getByRole("region", { name: "SQL results" });
    expect(formAction).toBeInTheDocument();
    expect(
      actionGroup.compareDocumentPosition(editor) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(
      actionGroup.compareDocumentPosition(results) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(screen.getByTestId("sql-workspace")).toHaveAttribute(
      "data-hide-actions",
      "true",
    );
    fireEvent.click(formAction);
    expect(structureActionMock).toHaveBeenCalledOnce();
  });

  it("REQ-FE-063: starts from a relation-free draft and saves through the shared SQL workspace", async () => {
    render(() => <SpaceQueryCreateRoute />);

    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => expect(editor).toHaveValue("SELECT 1"));
    expect(screen.queryByLabelText("Form for this query")).not
      .toBeInTheDocument();
    expect(screen.queryByLabelText("Aggregate")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Field")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Filter field")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Generate SQL from Form" }))
      .not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Generate aggregate SQL" }))
      .not.toBeInTheDocument();
    expect(screen.getByRole("group", { name: "SQL" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Form" })).toBeInTheDocument();
    fireEvent.input(editor, { target: { value: "SELECT 1" } });
    fireEvent.input(screen.getByLabelText("Query name"), {
      target: { value: "Relation-free draft" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(sqlCreateMock).toHaveBeenCalledWith("default", {
        name: "Relation-free draft",
        kind: "user-query",
        metadata: undefined,
        sql: "SELECT 1",
        variables: [],
      });
    });
    expect(navigateMock).toHaveBeenCalledWith("/spaces/default/sql");
  });

  it("normalizes template variables to the native SQL query placeholder", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => {
      expect(editor).toHaveValue("SELECT 1");
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

  it("saves an explicitly enabled default for a SQL variable", async () => {
    render(() => <SpaceQueryCreateRoute />);

    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => expect(editor).toHaveValue("SELECT 1"));
    fireEvent.input(editor, { target: { value: "SELECT $label" } });

    fireEvent.click(
      screen.getByRole("checkbox", { name: "Default for label" }),
    );
    fireEvent.input(
      screen.getByRole("textbox", { name: "Default for label" }),
      { target: { value: "draft" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(sqlCreateMock).toHaveBeenCalledWith(
        "default",
        expect.objectContaining({
          sql: "SELECT $label",
          variables: [{
            type: "string",
            name: "label",
            description: "",
            default: "draft",
          }],
        }),
      );
    });
  });

  it("leaves session semantic validation to the backend", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => {
      expect(editor).toHaveValue("SELECT 1");
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
