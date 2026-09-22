import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SpaceQueryCreateRoute from "./new";

const { navigateMock, formApiListMock, sqlCreateMock } = vi.hoisted(() => ({
  navigateMock: vi.fn(),
  formApiListMock: vi.fn(),
  sqlCreateMock: vi.fn(),
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

vi.mock("~/components", () => ({
  SqlQueryEditor: (props: {
    id?: string;
    value: string;
    onChange: (value: string) => void;
    onDiagnostics?: (
      diagnostics: Array<{
        from: number;
        to: number;
        severity: "error";
        message: string;
      }>,
    ) => void;
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
      <textarea
        id={props.id}
        aria-label="SQL"
        value={props.value}
        onInput={(event) => {
          const value = event.currentTarget.value;
          props.onChange(value);
          emitBackendDiagnostic(value);
        }}
      />
    );
  },
}));

vi.mock("~/lib/ugoite-client", () => ({
  formApi: { list: formApiListMock },
  sqlApi: { create: sqlCreateMock },
}));

describe("/spaces/:space_id/queries/new", () => {
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
    ]);
    sqlCreateMock.mockResolvedValue({ id: "query-1", revisionId: "rev-1" });
  });

  it("REQ-FE-063: uses a runnable Form relation starter with session ordering", async () => {
    render(() => <SpaceQueryCreateRoute />);

    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => {
      expect(editor).toHaveValue(
        'SELECT * FROM "form_entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 50',
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
          'SELECT * FROM "form_entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 50',
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
        'SELECT * FROM "form_entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 50',
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

  it("leaves session semantic validation to the backend", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => {
      expect(editor).toHaveValue(
        'SELECT * FROM "form_entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 50',
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

    const editor = await screen.findByRole("textbox", { name: "SQL" });
    fireEvent.input(editor, {
      target: { value: "SELECT * FROM missing_relation LIMIT $page_size" },
    });
    expect(await screen.findByText(/Line 1, column \d+: /))
      .toBeInTheDocument();
  });
});
