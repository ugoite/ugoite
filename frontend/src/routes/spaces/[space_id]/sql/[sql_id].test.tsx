import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import SpaceSqlDetailRoute from "./[sql_id]/index";
import { SqlArgumentDefinitionsEditor } from "~/components/SqlArgumentDefinitionsEditor";
import { formatDateLabel } from "~/lib/date-format";
import {
  resetMockData,
  seedForm,
  seedSpace,
  seedSqlEntry,
} from "~/test/mocks/handlers";
import type { Space } from "~/lib/types";
import { sqlApi } from "~/lib/ugoite-client";
import * as ugoiteClient from "~/lib/ugoite-client";
import type { SqlVariable } from "~/lib/types";
import { expectBackLinkAtHeaderStart } from "~/test/back-link-placement";

const navigateMock = vi.fn();
const entryRelation = "form_00000000000000000000000000000001";

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
  useParams: () => ({ space_id: "default", sql_id: "saved-query" }),
}));

vi.mock("~/components/SQLWorkspace", () => ({
  SQLWorkspace: (props: {
    id?: string;
    sql: string;
    readOnly?: boolean;
    previewDisabled?: boolean;
    planner?: { status: string; isCurrent: boolean };
    variables: Array<{ name: string; type: string; default?: unknown }>;
    parameterValues: Record<string, string | number | boolean | undefined>;
    onSqlChange?: (value: string) => void;
    onParameterChange?: (name: string, value: string) => void;
    onPreview: () => void;
    onCount?: () => void;
    onNext?: () => void;
    preview: { status: string };
  }) => (
    <div>
      <textarea
        id={props.id}
        data-testid="sql-editor"
        data-disabled={String(Boolean(props.readOnly))}
        aria-label="SQL"
        value={props.sql}
        readOnly={props.readOnly}
        onInput={(event) => props.onSqlChange?.(event.currentTarget.value)}
      />
      {props.variables.map((variable) => (
        <label>
          {variable.name}
          <input
            aria-label={variable.name}
            value={Object.prototype.hasOwnProperty.call(
                props.parameterValues,
                variable.name,
              )
              ? String(props.parameterValues[variable.name] ?? "")
              : String(variable.default ?? "")}
            readOnly={props.readOnly}
            onInput={(event) =>
              props.onParameterChange?.(
                variable.name,
                event.currentTarget.value,
              )}
          />
        </label>
      ))}
      <button
        type="button"
        disabled={props.previewDisabled ||
          props.planner?.status !== "success" ||
          !props.planner?.isCurrent}
        onClick={props.onPreview}
      >
        Run Query
      </button>
      <button type="button" onClick={props.onCount}>Count rows</button>
      <button type="button" onClick={props.onNext}>Next</button>
      <output>{props.preview.status}</output>
    </div>
  ),
}));

vi.mock("~/components/SqlStructureToolbar", () => ({
  SqlStructureToolbar: (props: {
    sql: () => string;
    argumentDefinitions?: () => readonly SqlVariable[];
    onArgumentAdded?: (variable: SqlVariable) => boolean | void;
    onArgumentUpdated?: (
      oldName: string,
      variable: SqlVariable,
    ) => boolean | void | Promise<boolean | void>;
    onArgumentRemoved?: (name: string) => boolean | void;
    argumentDefaults?: () => Record<string, { enabled: boolean; raw: string }>;
    onArgumentDefaultsChange?: (
      defaults: Record<string, { enabled: boolean; raw: string }>,
    ) => void;
    disabled?: () => boolean;
  }) => (
    <section role="dialog" aria-label="Arguments">
      <SqlArgumentDefinitionsEditor
        idPrefix="saved-sql-test"
        variables={props.argumentDefinitions?.() ?? []}
        referencedNames={props.argumentDefinitions?.().filter((variable) =>
          props.sql().includes(`$${variable.name}`) ||
          props.sql().includes(`{{${variable.name}}}`)
        ).map((variable) => variable.name) ?? []}
        defaults={props.argumentDefaults?.() ?? {}}
        disabled={props.disabled?.()}
        onAdd={(variable) => {
          if (!props.onArgumentAdded) return false;
          return props.onArgumentAdded(variable) === false ? false : true;
        }}
        onUpdate={(oldName, variable) =>
          props.onArgumentUpdated?.(oldName, variable) ?? false}
        onRemove={(name) => props.onArgumentRemoved?.(name) ?? false}
        onDefaultsChange={(defaults) =>
          props.onArgumentDefaultsChange?.(defaults)}
      />
    </section>
  ),
}));

describe("/spaces/:space_id/sql/:sql_id", () => {
  const testSpace: Space = {
    space_uid: "default",
    name: "Default",
    created_at: "2025-01-01T00:00:00Z",
  };

  afterEach(() => vi.restoreAllMocks());

  beforeEach(() => {
    navigateMock.mockReset();
    resetMockData();
    seedSpace(testSpace);
  });

  it("REQ-UX-NAV-001: exposes exactly one back control to saved SQL", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Recent Search",
      kind: "user-query",
      sql:
        `SELECT * FROM "${entryRelation}" WHERE owner = {{owner}} ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 10`,
      variables: [],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });

    render(() => <SpaceSqlDetailRoute />);

    const backLink = await screen.findByRole("link", {
      name: "Back to Saved SQL",
    });
    expect(backLink).toHaveAttribute("href", "/spaces/default/sql");
    expect(screen.getAllByRole("link", { name: "Back to Saved SQL" }))
      .toHaveLength(1);
    expectBackLinkAtHeaderStart(backLink);
    // Shell destinations stay in the shell: no route-level shortcuts.
    expect(screen.queryByRole("link", { name: "Back to Dashboard" }))
      .not.toBeInTheDocument();
  });

  it("REQ-FE-062: saved SQL detail renders an editable query workspace and supported actions", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Recent Search",
      kind: "user-query",
      sql:
        `SELECT * FROM "${entryRelation}" WHERE owner = {{owner}} ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 10`,
      variables: [{
        name: "owner",
        type: "string",
        description: "Owner filter",
      }],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });

    render(() => <SpaceSqlDetailRoute />);

    expect(await screen.findByRole("heading", { name: "Recent Search" }))
      .toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByTestId("sql-editor")).toHaveAttribute(
        "data-disabled",
        "false",
      )
    );
    expect(screen.getByText(formatDateLabel("2025-03-02T00:00:00Z")))
      .toBeInTheDocument();
    expect(screen.getByText(formatDateLabel("2025-03-01T00:00:00Z")))
      .toBeInTheDocument();
    expect(screen.getByTestId("sql-editor")).toHaveAttribute(
      "data-disabled",
      "false",
    );
    expect(screen.getByRole("textbox", { name: "SQL" })).toHaveValue(
      `SELECT * FROM "${entryRelation}" WHERE owner = {{owner}} ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 10`,
    );
    expect(await screen.findByRole("link", { name: "Open Variables" }))
      .toHaveAttribute(
        "href",
        "/spaces/default/sql/saved-query/variables",
      );
    expect(screen.getByRole("link", { name: "Back to Saved SQL" }))
      .toHaveAttribute(
        "href",
        "/spaces/default/sql",
      );
    // Shell destinations stay in the shell: no route-level shortcuts.
    expect(screen.queryByRole("link", { name: "Open Search" }))
      .not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Back to Dashboard" }))
      .not.toBeInTheDocument();
  });

  it("REQ-FE-062: keeps Form IDs behind technical details while showing binding status", async () => {
    const plan = vi.spyOn(sqlApi, "plan").mockResolvedValue({
      result_schema: [],
    });
    seedForm("default", {
      id: "form-stable-id",
      name: "RenamedExpense",
      sql_relation: entryRelation,
      version: 1,
      template: "# Expense\n",
      fields: {},
    });
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Expenses",
      kind: "user-query",
      sql: 'SELECT * FROM "Expense"',
      variables: [],
      metadata: {
        bindingVersion: 1,
        formBindings: [{ name: "Expense", formId: "form-stable-id" }],
      },
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });

    render(() => <SpaceSqlDetailRoute />);

    expect(
      await screen.findByText(
        "Saved as Expense; current name is RenamedExpense",
      ),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(plan).toHaveBeenCalledWith("default", {
        sql: 'SELECT * FROM "Expense"',
        parameters: {},
        parameter_types: {},
        binding_base: { id: "saved-query", revision_id: "rev-1" },
        form_binding_guards: [{ name: "Expense", form_id: "form-stable-id" }],
      }, expect.any(AbortSignal))
    );
    const bindingStatus = screen.getByRole("region", { name: "Bound Forms" });
    expect(within(bindingStatus).queryByText("form-stable-id")).toBeNull();
    expect(within(bindingStatus).queryByRole("definition")).toBeNull();

    fireEvent.click(
      within(bindingStatus).getByText("Technical details"),
    );
    await waitFor(() => {
      expect(within(bindingStatus).getByText("form-stable-id")).toBeVisible();
      expect(within(bindingStatus).getByRole("definition")).toHaveTextContent(
        "form-stable-id",
      );
    });
  });

  it("REQ-FE-062: saved SQL detail routes variable-free queries to stateless results", async () => {
    seedForm("default", {
      name: "Entry",
      sql_relation: entryRelation,
      version: 1,
      template: "# Entry\n",
      fields: {},
    });
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Runnable Query",
      kind: "user-query",
      sql:
        `SELECT * FROM "${entryRelation}" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 1`,
      variables: [],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });
    render(() => <SpaceSqlDetailRoute />);
    const previewButton = await screen.findByRole("button", {
      name: "Run Query",
    });
    await waitFor(() => expect(previewButton).toBeEnabled());
    const resultsButton = await screen.findByRole("button", {
      name: "SQL results",
    });
    fireEvent.click(resultsButton);

    expect(navigateMock).toHaveBeenCalledWith(
      "/spaces/default/sql/saved-query/run",
    );
  });

  it("previews the pinned Saved SQL revision with typed transient parameters", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Runnable Query",
      kind: "user-query",
      sql: "SELECT * FROM records WHERE id = $id",
      variables: [{ type: "integer", name: "id", description: "Entry ID" }],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });
    const query = vi.spyOn(sqlApi, "query").mockResolvedValue({
      columns: ["id"],
      rows: [[42]],
      has_more: true,
      next: "next-cursor",
      result_schema: [{ name: "id", type: "integer" }],
      exact_integer_rows: [["42"]],
    });
    const count = vi.spyOn(sqlApi, "count").mockResolvedValue(25);

    render(() => <SpaceSqlDetailRoute />);
    const parameter = await screen.findByRole("textbox", { name: "id" });
    const runButton = screen.getByRole("button", { name: "Run Query" });
    await waitFor(() => expect(runButton).toBeEnabled());
    fireEvent.input(parameter, { target: { value: "42" } });
    fireEvent.click(runButton);

    await waitFor(() => {
      expect(query).toHaveBeenCalledWith("default", {
        sql: "SELECT * FROM records WHERE id = $id",
        parameters: { id: 42 },
        parameter_types: { id: "integer" },
        include_exact_integer_values: true,
        limit: 100,
        saved_sql: { id: "saved-query", revision_id: "rev-1" },
      }, expect.any(AbortSignal));
    });
    expect(await screen.findByText("success")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await waitFor(() => expect(query).toHaveBeenCalledTimes(2));
    expect(query).toHaveBeenNthCalledWith(2, "default", {
      sql: "SELECT * FROM records WHERE id = $id",
      parameters: { id: 42 },
      parameter_types: { id: "integer" },
      include_exact_integer_values: true,
      limit: 100,
      continuation: "next-cursor",
      saved_sql: { id: "saved-query", revision_id: "rev-1" },
    }, expect.any(AbortSignal));

    fireEvent.click(screen.getByRole("button", { name: "Count rows" }));
    await waitFor(() => expect(count).toHaveBeenCalledOnce());
    expect(count).toHaveBeenCalledWith("default", {
      sql: "SELECT * FROM records WHERE id = $id",
      parameters: { id: 42 },
      parameter_types: { id: "integer" },
      saved_sql: { id: "saved-query", revision_id: "rev-1" },
    }, expect.any(AbortSignal));
    expect(navigateMock).not.toHaveBeenCalled();
    query.mockRestore();
    count.mockRestore();
  });

  it("rejects integer parameters that JavaScript cannot represent exactly", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Runnable Query",
      kind: "user-query",
      sql: "SELECT * FROM records WHERE id = $id",
      variables: [{ type: "integer", name: "id", description: "Entry ID" }],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });
    const query = vi.spyOn(sqlApi, "query");

    render(() => <SpaceSqlDetailRoute />);
    fireEvent.input(await screen.findByRole("textbox", { name: "id" }), {
      target: { value: "9007199254740993" },
    });
    const runButton = screen.getByRole("button", { name: "Run Query" });
    await waitFor(() => expect(runButton).toBeEnabled());
    fireEvent.click(runButton);

    await waitFor(() => expect(screen.getByText("error")).toBeInTheDocument());
    expect(query).not.toHaveBeenCalled();
    query.mockRestore();
  });

  it("previews API-projected typed defaults without adding transient values", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Runnable Query",
      kind: "user-query",
      sql: "SELECT * FROM records WHERE id = $id",
      variables: [{
        type: "integer",
        name: "id",
        description: "Entry ID",
        default: 0,
      }],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });
    const query = vi.spyOn(sqlApi, "query").mockResolvedValueOnce({
      columns: ["id"],
      rows: [[0]],
      has_more: false,
      result_schema: [{ name: "id", type: "integer" }],
      exact_integer_rows: [["0"]],
    });

    render(() => <SpaceSqlDetailRoute />);
    expect(await screen.findByRole("textbox", { name: "id" })).toHaveValue("0");
    const runButton = screen.getByRole("button", { name: "Run Query" });
    await waitFor(() => expect(runButton).toBeEnabled());
    fireEvent.click(runButton);

    await waitFor(() => {
      expect(query).toHaveBeenCalledWith("default", {
        sql: "SELECT * FROM records WHERE id = $id",
        parameters: { id: 0 },
        parameter_types: { id: "integer" },
        include_exact_integer_values: true,
        limit: 100,
        saved_sql: { id: "saved-query", revision_id: "rev-1" },
      }, expect.any(AbortSignal));
    });
    query.mockRestore();
  });

  it("plans and saves an argument type change as a draft while keeping exact revision previews", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Typed query",
      kind: "user-query",
      sql: "SELECT $value",
      variables: [{ name: "value", type: "integer", description: "" }],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });
    const plan = vi.spyOn(sqlApi, "plan");
    const update = vi.spyOn(sqlApi, "update").mockResolvedValue({
      id: "saved-query",
      revisionId: "rev-2",
    });
    const query = vi.spyOn(sqlApi, "query").mockResolvedValue({
      columns: ["value"],
      rows: [[null]],
      has_more: false,
    });
    render(() => <SpaceSqlDetailRoute />);
    fireEvent.change(
      await screen.findByRole("combobox", { name: "Type for value" }),
      { target: { value: "string" } },
    );
    await waitFor(() =>
      expect(plan).toHaveBeenCalledWith("default", {
        sql: "SELECT $value",
        parameters: { value: null },
        parameter_types: { value: "string" },
        binding_base: { id: "saved-query", revision_id: "rev-1" },
      }, expect.any(AbortSignal))
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Save" })).toBeEnabled()
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(update).toHaveBeenCalledWith(
        "default",
        "saved-query",
        expect.objectContaining({
          sql: "SELECT $value",
          variables: [{ name: "value", type: "string", description: "" }],
          parent_revision_id: "rev-1",
        }),
      )
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Run Query" })).toBeEnabled()
    );
    fireEvent.click(screen.getByRole("button", { name: "Run Query" }));
    await waitFor(() =>
      expect(query).toHaveBeenCalledWith(
        "default",
        expect.objectContaining({
          saved_sql: { id: "saved-query", revision_id: "rev-2" },
          parameter_types: { value: "string" },
        }),
        expect.any(AbortSignal),
      )
    );
  });

  it("pins an edited query preview to the revision returned by Save", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Planning",
      kind: "user-query",
      sql: "SELECT 1",
      variables: [],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });
    const update = vi.spyOn(sqlApi, "update").mockResolvedValueOnce({
      id: "saved-query",
      revisionId: "rev-2",
    });
    const query = vi.spyOn(sqlApi, "query").mockResolvedValueOnce({
      columns: ["value"],
      rows: [[2]],
      has_more: false,
      result_schema: [{ name: "value", type: "integer" }],
    });

    render(() => <SpaceSqlDetailRoute />);
    fireEvent.input(await screen.findByRole("textbox", { name: "SQL" }), {
      target: { value: "SELECT 2" },
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Save" })).toBeEnabled()
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(update).toHaveBeenCalledOnce());
    const runButton = screen.getByRole("button", { name: "Run Query" });
    await waitFor(() => expect(runButton).toBeEnabled());
    fireEvent.click(runButton);

    await waitFor(() => {
      expect(query).toHaveBeenCalledWith("default", {
        sql: "SELECT 2",
        parameters: {},
        parameter_types: {},
        include_exact_integer_values: true,
        limit: 100,
        saved_sql: { id: "saved-query", revision_id: "rev-2" },
      }, expect.any(AbortSignal));
    });
    update.mockRestore();
    query.mockRestore();
  });

  it("normalizes SQL variables and uses the returned revision for the next save", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Planning",
      kind: "user-query",
      sql: "SELECT 1",
      variables: [],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });
    const update = vi.spyOn(sqlApi, "update")
      .mockResolvedValueOnce({ id: "saved-query", revisionId: "rev-2" })
      .mockResolvedValueOnce({ id: "saved-query", revisionId: "rev-3" });

    render(() => <SpaceSqlDetailRoute />);
    const name = await screen.findByRole("textbox", { name: "Query name" });
    const editor = screen.getByRole("textbox", { name: "SQL" });
    fireEvent.input(name, { target: { value: "  Planning v2  " } });
    fireEvent.input(editor, {
      target: { value: "SELECT * FROM records WHERE title = {{title}}" },
    });
    expect(
      await screen.findByText("Add argument title before using it in SQL."),
    )
      .toBeInTheDocument();
    const dialog = screen.getByRole("dialog", { name: "Arguments" });
    fireEvent.input(within(dialog).getByLabelText("Argument name"), {
      target: { value: "title" },
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add argument" }),
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Save" })).toBeEnabled()
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(update).toHaveBeenCalledTimes(1));
    expect(update).toHaveBeenNthCalledWith(1, "default", "saved-query", {
      name: "Planning v2",
      kind: "user-query",
      metadata: undefined,
      sql: "SELECT * FROM records WHERE title = $title",
      variables: [{ type: "string", name: "title", description: "" }],
      parent_revision_id: "rev-1",
    });

    fireEvent.input(editor, {
      target: {
        value: "SELECT * FROM records WHERE title = {{title}} LIMIT 2",
      },
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Save" })).toBeEnabled()
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(update).toHaveBeenCalledTimes(2));
    expect(update).toHaveBeenNthCalledWith(
      2,
      "default",
      "saved-query",
      expect.objectContaining({ parent_revision_id: "rev-2" }),
    );
    update.mockRestore();
  });

  it("saves an explicitly configured typed variable default", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "By count",
      kind: "user-query",
      sql: "SELECT $count",
      variables: [{ type: "integer", name: "count", description: "Count" }],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });
    const update = vi.spyOn(sqlApi, "update").mockResolvedValue({
      id: "saved-query",
      revisionId: "rev-2",
    });

    render(() => <SpaceSqlDetailRoute />);
    fireEvent.click(
      await screen.findByRole("checkbox", { name: "Default for count" }),
    );
    fireEvent.input(
      screen.getByRole("textbox", { name: "Default for count" }),
      { target: { value: "12" } },
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Save" })).toBeEnabled()
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(update).toHaveBeenCalledWith("default", "saved-query", {
        name: "By count",
        kind: "user-query",
        metadata: undefined,
        sql: "SELECT $count",
        variables: [{
          type: "integer",
          name: "count",
          description: "Count",
          default: 12,
        }],
        parent_revision_id: "rev-1",
      });
    });
    update.mockRestore();
  });

  it("blocks preview and clears results for an invalid declared default until repaired", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Typed default",
      kind: "user-query",
      sql: "SELECT $count",
      variables: [{
        type: "integer",
        name: "count",
        default: 12,
        description: "",
      }],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });
    const query = vi.spyOn(sqlApi, "query");
    render(() => <SpaceSqlDetailRoute />);
    const preview = await screen.findByRole("button", {
      name: "Run Query",
    });
    await waitFor(() => expect(preview).toBeEnabled());
    const field = screen.getByRole("textbox", { name: "Default for count" });
    fireEvent.input(field, { target: { value: "abc" } });
    await waitFor(() => expect(preview).toBeDisabled());
    fireEvent.click(preview);
    expect(query).not.toHaveBeenCalled();
    expect(field).toHaveValue("abc");
    fireEvent.input(field, { target: { value: "24" } });
    await waitFor(() => expect(preview).toBeEnabled());
    query.mockRestore();
  });

  it("blocks saving a declared SQL argument while it is unused", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Unused declaration",
      kind: "user-query",
      sql: "SELECT 1",
      variables: [{ type: "integer", name: "unused", description: "" }],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });
    const update = vi.spyOn(sqlApi, "update");
    render(() => <SpaceSqlDetailRoute />);
    fireEvent.input(
      await screen.findByRole("textbox", { name: "Query name" }),
      {
        target: { value: "Changed name" },
      },
    );
    expect(await screen.findByText("Argument is not referenced in SQL."))
      .toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Save" })).toBeDisabled()
    );
    expect(update).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Run Query" }))
      .toBeDisabled();
    update.mockRestore();
  });

  it("renames Saved SQL parameter references through the Rust document endpoint", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "By count",
      kind: "user-query",
      sql: "SELECT $count",
      variables: [{ type: "integer", name: "count", description: "Count" }],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });
    const rename = vi.spyOn(ugoiteClient, "renameSqlDocumentParameter")
      .mockImplementation(async (source, oldName, newName) => ({
        contract_version: 26,
        status: "parsed",
        source: source.replaceAll(`$${oldName}`, `$${newName}`),
        diagnostics: [],
        tokens: [],
        nodes: [],
        conditions: [],
      }));
    const update = vi.spyOn(sqlApi, "update").mockResolvedValue({
      id: "saved-query",
      revisionId: "rev-2",
    });

    render(() => <SpaceSqlDetailRoute />);
    fireEvent.input(
      await screen.findByRole("textbox", { name: "Argument name for count" }),
      { target: { value: "total" } },
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Rename argument" }),
    );

    await waitFor(() => {
      expect(rename).toHaveBeenCalledWith("SELECT $count", "count", "total");
      expect(screen.getByRole("textbox", { name: "SQL" })).toHaveValue(
        "SELECT $total",
      );
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Save" })).toBeEnabled()
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(update).toHaveBeenCalledWith("default", "saved-query", {
        name: "By count",
        kind: "user-query",
        metadata: undefined,
        sql: "SELECT $total",
        variables: [{ type: "integer", name: "total", description: "Count" }],
        parent_revision_id: "rev-1",
      });
    });
    rename.mockRestore();
    update.mockRestore();
  });

  it("saves a blank name with Untitled metadata and surfaces stale conflicts", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Planning",
      kind: "user-query",
      sql: "SELECT 1",
      variables: [],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });
    const conflict = Object.assign(new Error("Revision conflict"), {
      status: 409,
      code: "REVISION_CONFLICT",
    });
    const update = vi.spyOn(sqlApi, "update").mockRejectedValue(conflict);

    render(() => <SpaceSqlDetailRoute />);
    const name = await screen.findByRole("textbox", { name: "Query name" });
    fireEvent.input(name, { target: { value: "   " } });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Save" })).toBeEnabled()
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(update).toHaveBeenCalledWith(
        "default",
        "saved-query",
        expect.objectContaining({
          name: null,
          metadata: { generatedName: "untitled" },
          parent_revision_id: "rev-1",
        }),
      )
    );
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(name).toHaveValue("   ");
    update.mockRestore();
  });

  it("deletes only after shared confirmation and returns to the Saved SQL list", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: "Planning",
      kind: "user-query",
      sql: "SELECT 1",
      variables: [],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });
    const remove = vi.spyOn(sqlApi, "delete").mockResolvedValue(undefined);

    render(() => <SpaceSqlDetailRoute />);
    fireEvent.click(
      await screen.findByRole("button", { name: "Delete query" }),
    );
    const dialog = await screen.findByRole("dialog", { name: "Delete query" });
    expect(remove).not.toHaveBeenCalled();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Delete query" }),
    );

    await waitFor(() =>
      expect(remove).toHaveBeenCalledWith("default", "saved-query")
    );
    expect(navigateMock).toHaveBeenCalledWith("/spaces/default/sql");
    remove.mockRestore();
  });

  it("keeps search-history detail read-only and retains its derived label", async () => {
    seedSqlEntry("default", {
      id: "saved-query",
      name: null,
      kind: "search-history",
      metadata: {
        searchCriteria: {
          formName: "Entry",
          tags: [],
          updatedFrom: "",
          updatedTo: "",
          fieldConditions: [],
        },
      },
      sql: "SELECT 1",
      variables: [],
      created_at: "2025-03-01T00:00:00Z",
      updated_at: "2025-03-02T00:00:00Z",
      revision_id: "rev-1",
    });

    render(() => <SpaceSqlDetailRoute />);

    expect(await screen.findByRole("heading", { name: /Advanced search/ }))
      .toBeInTheDocument();
    expect(screen.getByTestId("sql-editor")).toHaveAttribute("readonly");
    expect(screen.queryByRole("button", { name: "Save" }))
      .not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete query" }))
      .not.toBeInTheDocument();
  });
});
