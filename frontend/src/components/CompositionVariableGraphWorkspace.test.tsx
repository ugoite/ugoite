import "@testing-library/jest-dom/vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CompositionVariableGraphWorkspace } from "./CompositionVariableGraphWorkspace";
import {
  type CompositionGraphDraft,
  type CompositionVariableNode,
  createCompositionGraphDraft,
} from "~/lib/composition-variable-binding";
import type { CompositionVariableCreationKind } from "~/components/CompositionVariableCreationForm";
import * as sqlModule from "~/lib/sql";
import { setLocale } from "~/lib/i18n";
import type { PlannedSqlColumns } from "~/lib/sql-document-columns";

const { parseInputMock, renameVariableMock } = vi.hoisted(() => ({
  parseInputMock: vi.fn(),
  renameVariableMock: vi.fn(),
}));

const { buildSqlSchema } = sqlModule;
afterEach(() => vi.restoreAllMocks());

const creationProps = (
  draft: () => CompositionGraphDraft,
  setDraft: (next: CompositionGraphDraft) => void,
  setSelected: (id: string | null) => void,
) => {
  const [pendingKind, setPendingKind] = createSignal<
    CompositionVariableCreationKind | undefined
  >();
  const createNode = (kind: CompositionVariableCreationKind, id: string) =>
    kind === "constant"
      ? {
        id,
        kind,
        value_type: { scalar_type: "string" as const, nullable: false },
        value: { kind: "string" as const, value: "" },
      }
      : kind === "input"
      ? {
        id,
        kind,
        value_type: { scalar_type: "string" as const, nullable: false },
        required: true,
      }
      : {
        id,
        kind,
        returns: {
          kind: "scalar" as const,
          value_type: { scalar_type: "integer" as const, nullable: false },
        },
        query: { kind: "inline_sql" as const, sql: "SELECT 1", arguments: [] },
      };
  return {
    pendingCreationKind: pendingKind,
    onRequestCreation: setPendingKind,
    onCommitCreation: (name: string) => {
      const kind = pendingKind();
      const id = name.trim();
      if (!kind || !id || draft().variables.some((node) => node.id === id)) {
        return false;
      }
      const node = createNode(kind, id) as CompositionVariableNode;
      setDraft({ ...draft(), variables: [...draft().variables, node] });
      setSelected(id);
      setPendingKind(undefined);
      return true;
    },
    onCancelCreation: () => setPendingKind(undefined),
  };
};

vi.mock("~/lib/ugoite-client", () => ({
  lintSql: async (_sql: string, profile = "standard") => ({
    contract_version: 1,
    profile,
    diagnostics: [],
  }),
  parseVariableGraphInput: (...args: unknown[]) => parseInputMock(...args),
  renameCompositionVariable: (...args: unknown[]) =>
    renameVariableMock(...args),
}));

vi.mock("./SqlQueryEditor", () => ({
  SqlQueryEditor: (props: {
    id?: string;
    value: string;
    ariaLabel?: string;
    lintProfile?: "standard" | "composition";
    disabled?: boolean;
    additionalDiagnostics?: readonly { message: string }[];
    onChange: (value: string) => void;
  }) => (
    <textarea
      id={props.id}
      aria-label={props.ariaLabel}
      data-lint-profile={props.lintProfile ?? "standard"}
      data-planner-diagnostics={props.additionalDiagnostics?.map((item) =>
        item.message
      ).join("|") ?? ""}
      value={props.value}
      readOnly={props.disabled}
      onInput={(event) => props.onChange(event.currentTarget.value)}
    />
  ),
}));

describe("CompositionVariableGraphWorkspace", () => {
  it("retains an i64 default as exact decimal text while editing", async () => {
    setLocale("en");
    const maximum = "9223372036854775807";
    parseInputMock.mockReset().mockResolvedValue({
      status: "valid",
      value: maximum,
    });
    const [draft, setDraft] = createSignal<CompositionGraphDraft>({
      ...createCompositionGraphDraft("Report"),
      variables: [{
        id: "limit",
        kind: "input",
        value_type: { scalar_type: "integer", nullable: false },
        required: false,
      }],
    });
    render(() => (
      <CompositionVariableGraphWorkspace
        draft={draft()}
        selectedVariableId="limit"
        onSelect={() => {}}
        onDraftChange={setDraft}
        onRemove={() => {}}
        sqlSchema={buildSqlSchema([])}
      />
    ));

    fireEvent.input(screen.getByRole("textbox", { name: "Default" }), {
      target: { value: maximum },
    });
    await waitFor(() =>
      expect(draft().variables[0]).toMatchObject({
        kind: "input",
        default: { kind: "integer", value: maximum },
      })
    );
    expect(screen.getByRole("textbox", { name: "Default" })).toHaveValue(
      maximum,
    );
    parseInputMock.mockReset();
  });

  it("adds typed graph nodes through the variable rail", async () => {
    setLocale("en");
    const [draft, setDraft] = createSignal<CompositionGraphDraft>(
      createCompositionGraphDraft("Report"),
    );
    const [selected, setSelected] = createSignal<string | null>(null);
    const creation = creationProps(draft, setDraft, setSelected);
    const onDraftChange = vi.fn(setDraft);
    render(() => (
      <CompositionVariableGraphWorkspace
        draft={draft()}
        selectedVariableId={selected()}
        onSelect={setSelected}
        onDraftChange={onDraftChange}
        pendingCreationKind={creation.pendingCreationKind()}
        onRequestCreation={creation.onRequestCreation}
        onCommitCreation={creation.onCommitCreation}
        onCancelCreation={creation.onCancelCreation}
        onRemove={(id) =>
          setDraft({
            ...draft(),
            variables: draft().variables.filter((node) => node.id !== id),
          })}
        sqlSchema={buildSqlSchema([])}
      />
    ));

    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    fireEvent.click(screen.getByRole("button", { name: "Input" }));

    expect(draft().variables).toEqual([]);
    const name = screen.getByRole("textbox", { name: "Variable name" });
    await waitFor(() => expect(name).toHaveFocus());
    fireEvent.input(name, {
      target: { value: "search_term" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    expect(draft().variables).toEqual([{
      id: "search_term",
      kind: "input",
      value_type: { scalar_type: "string", nullable: false },
      required: true,
    }]);
    expect(selected()).toBe("search_term");
    expect(screen.getByRole("combobox", { name: "Type" })).toBeVisible();

    fireEvent.change(screen.getByRole("combobox", { name: "Type" }), {
      target: { value: "date" },
    });
    expect(draft().variables[0]).toMatchObject({
      id: "search_term",
      kind: "input",
      value_type: { scalar_type: "date", nullable: false },
    });
    expect(onDraftChange).toHaveBeenCalled();
  });

  it("ignores a pending default parse after removing and re-adding the same ID", async () => {
    setLocale("en");
    let resolveParse!: (value: unknown) => void;
    parseInputMock.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveParse = resolve;
      }),
    );
    const [draft, setDraft] = createSignal<CompositionGraphDraft>({
      ...createCompositionGraphDraft("Report"),
      variables: [{
        id: "input_1",
        kind: "input",
        value_type: { scalar_type: "integer", nullable: false },
        required: true,
      }],
    });
    const [, setSelected] = createSignal<string | null>("input_1");
    const creation = creationProps(draft, setDraft, setSelected);
    render(() => (
      <CompositionVariableGraphWorkspace
        draft={draft()}
        selectedVariableId="input_1"
        onSelect={setSelected}
        onDraftChange={setDraft}
        pendingCreationKind={creation.pendingCreationKind()}
        onRequestCreation={creation.onRequestCreation}
        onCommitCreation={creation.onCommitCreation}
        onCancelCreation={creation.onCancelCreation}
        onRemove={(id) =>
          setDraft((current) => ({
            ...current,
            variables: current.variables.filter((node) => node.id !== id),
          }))}
        sqlSchema={buildSqlSchema([])}
      />
    ));

    fireEvent.input(screen.getByRole("textbox", { name: "Default" }), {
      target: { value: "42" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Remove input_1" }));
    expect(draft().variables).toEqual([]);

    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    fireEvent.click(screen.getByRole("button", { name: "Input" }));
    fireEvent.input(screen.getByRole("textbox", { name: "Variable name" }), {
      target: { value: "input_1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    expect(draft().variables[0]).toMatchObject({
      id: "input_1",
      kind: "input",
      value_type: { scalar_type: "string", nullable: false },
      required: true,
    });

    resolveParse({ status: "valid", value: "42" });
    await Promise.resolve();

    expect(
      (draft().variables[0] as
        | Extract<CompositionVariableNode, { kind: "input" }>
        | undefined)?.default,
    ).toBeUndefined();
    expect(screen.getByRole("textbox", { name: "Default" })).toHaveValue("");
  });

  it("ignores a pending default parse after the input type changes", async () => {
    setLocale("en");
    let resolveParse!: (value: unknown) => void;
    parseInputMock.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveParse = resolve;
      }),
    );
    const [draft, setDraft] = createSignal<CompositionGraphDraft>({
      ...createCompositionGraphDraft("Report"),
      variables: [{
        id: "minimum",
        kind: "input",
        value_type: { scalar_type: "integer", nullable: false },
        required: true,
      }],
    });
    render(() => (
      <CompositionVariableGraphWorkspace
        draft={draft()}
        selectedVariableId="minimum"
        onSelect={() => {}}
        onDraftChange={setDraft}
        onRemove={() => {}}
        sqlSchema={buildSqlSchema([])}
      />
    ));

    fireEvent.input(screen.getByRole("textbox", { name: "Default" }), {
      target: { value: "42" },
    });
    fireEvent.change(screen.getByRole("combobox", { name: "Type" }), {
      target: { value: "float" },
    });
    resolveParse({ status: "valid", value: "42" });
    await Promise.resolve();

    expect(draft().variables[0]).toMatchObject({
      id: "minimum",
      kind: "input",
      value_type: { scalar_type: "float", nullable: false },
    });
    expect(
      (draft().variables[0] as
        | Extract<CompositionVariableNode, { kind: "input" }>
        | undefined)?.default,
    ).toBeUndefined();
    expect(screen.getByRole("textbox", { name: "Default" })).toHaveValue("");
  });

  it("commits a variable rename returned by the Rust Domain operation", async () => {
    setLocale("en");
    const [draft, setDraft] = createSignal<CompositionGraphDraft>(
      createCompositionGraphDraft("Report"),
    );
    const [selected, setSelected] = createSignal<string | null>(null);
    const creation = creationProps(draft, setDraft, setSelected);
    render(() => (
      <CompositionVariableGraphWorkspace
        draft={draft()}
        selectedVariableId={selected()}
        onSelect={setSelected}
        onDraftChange={setDraft}
        pendingCreationKind={creation.pendingCreationKind()}
        onRequestCreation={creation.onRequestCreation}
        onCommitCreation={creation.onCommitCreation}
        onCancelCreation={creation.onCancelCreation}
        onRemove={() => {}}
        sqlSchema={buildSqlSchema([])}
      />
    ));

    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    fireEvent.click(screen.getByRole("button", { name: "Input" }));
    fireEvent.input(screen.getByRole("textbox", { name: "Variable name" }), {
      target: { value: "month_start" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    expect(draft().variables[0]?.id).toBe("month_start");
    setDraft({
      ...draft(),
      components: [{
        id: "text_1",
        kind: "text",
        content: "Start: {{month_start}}",
        style: "body",
      }],
      layout: {
        kind: "flow",
        rows: [{
          id: "main",
          items: [{ kind: "component", component: "text_1" }],
        }],
      },
    });
    renameVariableMock.mockResolvedValueOnce({
      variables: [{
        id: "period_start",
        kind: "input",
        value_type: { scalar_type: "date", nullable: false },
        required: true,
      }],
      components: [{
        id: "text_1",
        kind: "text",
        content: "Start: {{period_start}}",
        style: "body",
      }],
    });
    fireEvent.input(screen.getByRole("textbox", { name: "Variable name" }), {
      target: { value: "period_start" },
    });

    await waitFor(() => {
      expect(draft().variables[0]?.id).toBe("period_start");
      expect(draft().components[0]).toMatchObject({
        kind: "text",
        content: "Start: {{period_start}}",
      });
    });
    expect(selected()).toBe("period_start");
    expect(screen.getByRole("button", { name: "period_start" }))
      .toBeVisible();
  });

  it("discards a stale asynchronous Domain rename response", async () => {
    setLocale("en");
    let resolveFirst!: (value: {
      variables: CompositionVariableNode[];
      components: CompositionGraphDraft["components"];
    }) => void;
    let resolveSecond!: (value: {
      variables: CompositionVariableNode[];
      components: CompositionGraphDraft["components"];
    }) => void;
    renameVariableMock.mockReset()
      .mockReturnValueOnce(new Promise((resolve) => resolveFirst = resolve))
      .mockReturnValueOnce(new Promise((resolve) => resolveSecond = resolve));
    const initial: CompositionGraphDraft = {
      ...createCompositionGraphDraft("Report"),
      variables: [{
        id: "date_value",
        kind: "input",
        value_type: { scalar_type: "date", nullable: false },
        required: true,
      }],
      components: [{
        id: "text_1",
        kind: "text",
        content: "Date: {{date_value}}",
        style: "body",
      }],
      layout: {
        kind: "flow",
        rows: [{
          id: "main",
          items: [{ kind: "component", component: "text_1" }],
        }],
      },
    };
    const [draft, setDraft] = createSignal(initial);
    render(() => (
      <CompositionVariableGraphWorkspace
        draft={draft()}
        selectedVariableId="date_value"
        onSelect={() => {}}
        onDraftChange={setDraft}
        onRemove={() => {}}
        sqlSchema={buildSqlSchema([])}
      />
    ));

    const name = screen.getByRole("textbox", { name: "Variable name" });
    fireEvent.input(name, { target: { value: "first_date" } });
    fireEvent.input(name, { target: { value: "latest_date" } });
    expect(renameVariableMock).toHaveBeenCalledTimes(2);

    const renamed = (to: string) => ({
      variables: [{
        id: to,
        kind: "input" as const,
        value_type: { scalar_type: "date" as const, nullable: false },
        required: true,
      }],
      components: [{
        id: "text_1",
        kind: "text" as const,
        content: `Date: {{${to}}}`,
        style: "body" as const,
      }],
    });
    resolveSecond(renamed("latest_date"));
    await waitFor(() => expect(draft().variables[0]?.id).toBe("latest_date"));
    resolveFirst(renamed("first_date"));
    await Promise.resolve();

    expect(draft().components[0]).toMatchObject({
      content: "Date: {{latest_date}}",
    });
    expect(draft().variables[0]?.id).toBe("latest_date");
  });

  it("keeps query shape selection enabled until planning starts and waits for its result", async () => {
    setLocale("en");
    const [draft, setDraft] = createSignal<CompositionGraphDraft>({
      ...createCompositionGraphDraft("Report"),
      variables: [{
        id: "rows",
        kind: "query",
        returns: { kind: "table", columns: [] },
        query: {
          kind: "inline_sql",
          sql: "SELECT 1 AS value",
          arguments: [],
        },
      }],
    });
    let finishPlan!: (plan: PlannedSqlColumns) => void;
    const onPlanColumnsForQuery = vi.fn(
      () =>
        new Promise<PlannedSqlColumns>((resolve) => {
          finishPlan = resolve;
        }),
    );
    render(() => (
      <CompositionVariableGraphWorkspace
        draft={draft()}
        selectedVariableId="rows"
        onSelect={() => {}}
        onDraftChange={setDraft}
        onRemove={() => {}}
        sqlSchema={buildSqlSchema([])}
        onPlanColumnsForQuery={onPlanColumnsForQuery}
      />
    ));

    const resultShape = screen.getByRole("combobox", {
      name: "Query result",
    });
    expect(resultShape).toBeEnabled();
    fireEvent.change(resultShape, { target: { value: "scalar" } });
    await waitFor(() => expect(resultShape).toBeDisabled());
    expect(onPlanColumnsForQuery).toHaveBeenCalledOnce();
    expect(onPlanColumnsForQuery).toHaveBeenCalledWith("rows");

    finishPlan({
      columns: ["value"],
      result_columns: [{
        name: "value",
        value_type: { scalar_type: "integer", nullable: false },
      }],
      isCurrent: () => true,
    });
    await waitFor(() => {
      expect(resultShape).toBeEnabled();
      expect(resultShape).toHaveValue("scalar");
    });
    expect(draft().variables[0]).toMatchObject({
      id: "rows",
      kind: "query",
      returns: {
        kind: "scalar",
        value_type: { scalar_type: "integer", nullable: false },
      },
    });
  });

  it("keeps SQL lint, parameter bindings, and execution errors local to the selected query", async () => {
    setLocale("en");
    const runQuery = vi.fn();
    const [draft, setDraft] = createSignal<CompositionGraphDraft>({
      ...createCompositionGraphDraft("Report"),
      variables: [
        {
          id: "floor",
          kind: "constant",
          value_type: { scalar_type: "integer", nullable: false },
          value: { kind: "integer", value: "1" },
        },
        {
          id: "ceiling",
          kind: "constant",
          value_type: { scalar_type: "integer", nullable: false },
          value: { kind: "integer", value: "10" },
        },
        {
          id: "rows",
          kind: "query",
          returns: { kind: "table", columns: [] },
          query: {
            kind: "inline_sql",
            sql: "SELECT {{minimum}}",
            arguments: [{ name: "minimum", variable: "floor" }],
          },
        },
      ],
    });
    render(() => (
      <CompositionVariableGraphWorkspace
        draft={draft()}
        selectedVariableId="rows"
        onSelect={() => {}}
        onDraftChange={setDraft}
        onRemove={() => {}}
        sqlSchema={buildSqlSchema([])}
        previewForQuery={() => ({
          status: "error",
          isCurrent: true,
          error: "Query execution failed",
        })}
        onRunQuery={runQuery}
        onQueryChange={(id, sql) =>
          setDraft((current) => ({
            ...current,
            variables: current.variables.map((node) =>
              node.id === id && node.kind === "query"
                ? { ...node, query: { ...node.query, kind: "inline_sql", sql } }
                : node
            ),
          }))}
      />
    ));
    const editor = screen.getByRole("textbox", { name: "SQL" });
    expect(editor).toHaveValue("SELECT {{minimum}}");
    expect(editor).toHaveAttribute("data-lint-profile", "composition");
    const queryWorkspace = editor.closest(
      ".sql-workspace",
    ) as HTMLElement | null;
    const runQueryButton = screen.getByRole("button", { name: "Run Query" });
    await waitFor(() => expect(runQueryButton).toBeEnabled(), {
      timeout: 10000,
    });
    expect(within(queryWorkspace!).getByRole("alert")).toHaveTextContent(
      "Query execution failed",
    );
    fireEvent.click(runQueryButton);
    expect(runQuery).toHaveBeenCalledWith("rows");

    const binding = screen.getByRole("combobox", {
      name: "Choose a variable for minimum",
    });
    fireEvent.change(binding, { target: { value: "ceiling" } });
    await waitFor(() => expect(binding).toHaveValue("ceiling"));
    const editedSql = "SELECT {{minimum}} + 1 AS amount";
    setDraft((current) => ({
      ...current,
      variables: current.variables.map((node) =>
        node.id === "rows" && node.kind === "query"
          ? {
            ...node,
            query: { ...node.query, kind: "inline_sql", sql: editedSql },
          }
          : node
      ),
    }));
    await waitFor(() => expect(editor).toHaveValue(editedSql));
    fireEvent.change(binding, { target: { value: "floor" } });
    expect(draft().variables.find((node) => node.id === "rows")).toMatchObject({
      query: {
        sql: editedSql,
        arguments: [{ name: "minimum", variable: "floor" }],
      },
    });
    setDraft((current) => ({
      ...current,
      variables: [...current.variables, {
        id: "next_floor",
        kind: "constant",
        value_type: { scalar_type: "integer", nullable: false },
        value: { kind: "integer", value: "2" },
      }],
    }));
    expect(within(binding).getByRole("option", { name: "next_floor" }))
      .toBeInTheDocument();
  });

  it("connects one same-named scalar SQL marker and keeps an explicit unbind", async () => {
    setLocale("en");
    const [draft, setDraft] = createSignal<CompositionGraphDraft>({
      ...createCompositionGraphDraft("Report"),
      variables: [
        {
          id: "month",
          kind: "constant",
          value_type: { scalar_type: "integer", nullable: false },
          value: { kind: "integer", value: "11" },
        },
        {
          id: "rows",
          kind: "query",
          returns: { kind: "table", columns: [] },
          query: {
            kind: "inline_sql",
            sql: "SELECT 1",
            arguments: [],
          },
        },
      ],
    });
    render(() => (
      <CompositionVariableGraphWorkspace
        draft={draft()}
        selectedVariableId="rows"
        onSelect={() => {}}
        onDraftChange={setDraft}
        onRemove={() => {}}
        sqlSchema={buildSqlSchema([])}
        onQueryChange={(id, sql, _guards, queryArguments) =>
          setDraft((current) => ({
            ...current,
            variables: current.variables.map((node) =>
              node.id === id && node.kind === "query"
                ? {
                  ...node,
                  query: {
                    ...node.query,
                    kind: "inline_sql",
                    sql,
                    ...(queryArguments
                      ? { arguments: queryArguments }
                      : {}),
                  },
                }
                : node
            ),
          }))}
      />
    ));

    fireEvent.input(screen.getByRole("textbox", { name: "SQL" }), {
      target: { value: "SELECT {{month}}" },
    });
    await waitFor(() => {
      expect(draft().variables.find((node) => node.id === "rows")).toMatchObject({
        query: {
          arguments: [{ name: "month", variable: "month" }],
        },
      });
    });

    const binding = screen.getByRole("combobox", {
      name: "Choose a variable for month",
    });
    fireEvent.change(binding, { target: { value: "" } });
    await waitFor(() => {
      expect(draft().variables.find((node) => node.id === "rows")).toMatchObject({
        query: { arguments: [] },
      });
    });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(draft().variables.find((node) => node.id === "rows")).toMatchObject({
      query: { arguments: [] },
    });
  });

  it("keeps explicit marker unbinds independent across selected SQL queries", async () => {
    setLocale("en");
    const [draft, setDraft] = createSignal<CompositionGraphDraft>({
      ...createCompositionGraphDraft("Report"),
      variables: [
        {
          id: "month",
          kind: "constant",
          value_type: { scalar_type: "integer", nullable: false },
          value: { kind: "integer", value: "11" },
        },
        ...["rows_a", "rows_b"].map((id) => ({
          id,
          kind: "query" as const,
          returns: { kind: "table" as const, columns: [] },
          query: {
            kind: "inline_sql" as const,
            sql: "SELECT 1",
            arguments: [],
          },
        })),
      ],
    });
    const [selected, setSelected] = createSignal<string | null>("rows_a");
    render(() => (
      <CompositionVariableGraphWorkspace
        draft={draft()}
        selectedVariableId={selected()}
        onSelect={setSelected}
        onDraftChange={setDraft}
        onRemove={(id) =>
          setDraft((current) => ({
            ...current,
            variables: current.variables.filter((node) => node.id !== id),
          }))}
        sqlSchema={buildSqlSchema([])}
        onQueryChange={(id, sql, _guards, queryArguments) =>
          setDraft((current) => ({
            ...current,
            variables: current.variables.map((node) =>
              node.id === id && node.kind === "query"
                ? {
                  ...node,
                  query: {
                    ...node.query,
                    kind: "inline_sql",
                    sql,
                    ...(queryArguments ? { arguments: queryArguments } : {}),
                  },
                }
                : node
            ),
          }))}
      />
    ));

    const sql = screen.getByRole("textbox", { name: "SQL" });
    fireEvent.input(sql, { target: { value: "SELECT {{month}}" } });
    await waitFor(() => {
      expect(draft().variables.find((node) => node.id === "rows_a"))
        .toMatchObject({ query: { arguments: [{ name: "month", variable: "month" }] } });
    });
    const binding = screen.getByRole("combobox", {
      name: "Choose a variable for month",
    });
    fireEvent.change(binding, { target: { value: "" } });
    await waitFor(() => {
      expect(draft().variables.find((node) => node.id === "rows_a"))
        .toMatchObject({ query: { arguments: [] } });
    });

    fireEvent.click(screen.getByRole("button", { name: "rows_b" }));
    await waitFor(() => expect(sql).toHaveValue("SELECT 1"));
    fireEvent.input(sql, { target: { value: "SELECT {{month}}" } });
    await waitFor(() => {
      expect(draft().variables.find((node) => node.id === "rows_b"))
        .toMatchObject({ query: { arguments: [{ name: "month", variable: "month" }] } });
    });

    fireEvent.click(screen.getByRole("button", { name: "rows_a" }));
    await waitFor(() => expect(sql).toHaveValue("SELECT {{month}}"));
    expect(screen.getByRole("combobox", {
      name: "Choose a variable for month",
    })).toHaveValue("");
    expect(draft().variables.find((node) => node.id === "rows_b"))
      .toMatchObject({ query: { arguments: [{ name: "month", variable: "month" }] } });

    fireEvent.input(sql, { target: { value: "SELECT {{month" } });
    fireEvent.input(sql, { target: { value: "SELECT {{month}}" } });
    await waitFor(() => {
      expect(screen.getByRole("combobox", {
        name: "Choose a variable for month",
      })).toHaveValue("");
    });
    expect(draft().variables.find((node) => node.id === "rows_a"))
      .toMatchObject({ query: { arguments: [] } });

    fireEvent.input(sql, { target: { value: "SELECT 1" } });
    await waitFor(() => {
      expect(draft().variables.find((node) => node.id === "rows_a"))
        .toMatchObject({ query: { arguments: [] } });
    });
    await waitFor(() => {
      expect(screen.queryByRole("combobox", {
        name: "Choose a variable for month",
      })).not.toBeInTheDocument();
    });
    fireEvent.input(sql, { target: { value: "SELECT {{month}}" } });
    await waitFor(() => {
      expect(draft().variables.find((node) => node.id === "rows_a"))
        .toMatchObject({ query: { arguments: [{ name: "month", variable: "month" }] } });
    });

    const reconnect = screen.getByRole("combobox", {
      name: "Choose a variable for month",
    });
    fireEvent.change(reconnect, { target: { value: "" } });
    await waitFor(() => expect(reconnect).toHaveValue(""));
    fireEvent.change(reconnect, { target: { value: "month" } });
    await waitFor(() => {
      expect(draft().variables.find((node) => node.id === "rows_a"))
        .toMatchObject({ query: { arguments: [{ name: "month", variable: "month" }] } });
    });

    fireEvent.change(reconnect, { target: { value: "" } });
    await waitFor(() => expect(reconnect).toHaveValue(""));
    setSelected("rows_b");
    setDraft((current) => ({
      ...current,
      variables: current.variables.map((node) =>
        node.id === "rows_a" && node.kind === "query"
          ? { ...node, id: "rows_a_renamed", query: { ...node.query, arguments: [] } }
          : node
      ),
    }));
    setSelected("rows_a_renamed");
    await waitFor(() => {
      expect(draft().variables.find((node) => node.id === "rows_a_renamed"))
        .toMatchObject({ query: { arguments: [{ name: "month", variable: "month" }] } });
    });

    const renamedBinding = screen.getByRole("combobox", {
      name: "Choose a variable for month",
    });
    fireEvent.change(renamedBinding, { target: { value: "" } });
    await waitFor(() => expect(renamedBinding).toHaveValue(""));
    setSelected("rows_b");
    setDraft((current) => ({
      ...current,
      variables: current.variables.filter((node) => node.id !== "rows_a_renamed"),
    }));
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "rows_a_renamed" }))
        .not.toBeInTheDocument();
    });
    setDraft((current) => ({
      ...current,
      variables: [...current.variables, {
        id: "rows_a_renamed",
        kind: "query" as const,
        returns: { kind: "table" as const, columns: [] },
        query: {
          kind: "inline_sql" as const,
          sql: "SELECT {{month}}",
          arguments: [],
        },
      }],
    }));
    setSelected("rows_a_renamed");
    await waitFor(() => {
      expect(draft().variables.find((node) => node.id === "rows_a_renamed"))
        .toMatchObject({ query: { arguments: [{ name: "month", variable: "month" }] } });
    });
  });

  it("keeps a manual unbind through a delayed SQL normalization and query switch", async () => {
    setLocale("en");
    const [draft, setDraft] = createSignal<CompositionGraphDraft>({
      ...createCompositionGraphDraft("Report"),
      variables: [
        {
          id: "month",
          kind: "constant",
          value_type: { scalar_type: "integer", nullable: false },
          value: { kind: "integer", value: "11" },
        },
        ...["rows_a", "rows_b"].map((id) => ({
          id,
          kind: "query" as const,
          returns: { kind: "table" as const, columns: [] },
          query: {
            kind: "inline_sql" as const,
            sql: id === "rows_a" ? "SELECT {{month}}" : "SELECT 1",
            arguments: id === "rows_a"
              ? [{ name: "month", variable: "month" }]
              : [],
          },
        })),
      ],
    });
    const [selected, setSelected] = createSignal<string | null>("rows_a");
    render(() => (
      <CompositionVariableGraphWorkspace
        draft={draft()}
        selectedVariableId={selected()}
        onSelect={setSelected}
        onDraftChange={setDraft}
        onRemove={(id) =>
          setDraft((current) => ({
            ...current,
            variables: current.variables.filter((node) => node.id !== id),
          }))}
        sqlSchema={buildSqlSchema([])}
        onQueryChange={(id, sql, _guards, queryArguments) =>
          setDraft((current) => ({
            ...current,
            variables: current.variables.map((node) =>
              node.id === id && node.kind === "query"
                ? {
                  ...node,
                  query: {
                    ...node.query,
                    kind: "inline_sql",
                    sql,
                    ...(queryArguments ? { arguments: queryArguments } : {}),
                  },
                }
                : node
            ),
          }))}
      />
    ));

    const sql = screen.getByRole("textbox", { name: "SQL" });
    await waitFor(() => {
      expect(screen.getByRole("combobox", {
        name: "Choose a variable for month",
      })).toHaveValue("month");
    });
    fireEvent.change(screen.getByRole("combobox", {
      name: "Choose a variable for month",
    }), { target: { value: "" } });
    await waitFor(() => expect(screen.getByRole("combobox", {
      name: "Choose a variable for month",
    })).toHaveValue(""));

    const editedSql = "SELECT {{month}} + 1";
    let resolveNormalize!: (
      result: Awaited<ReturnType<typeof sqlModule.normalizeSqlDocument>>,
    ) => void;
    const normalizeSqlDocumentSpy = vi.spyOn(
      sqlModule,
      "normalizeSqlDocument",
    );
    normalizeSqlDocumentSpy.mockImplementationOnce(() =>
      new Promise((resolve) => {
        resolveNormalize = resolve;
      })
    );
    fireEvent.input(sql, { target: { value: editedSql } });
    await waitFor(() => expect(normalizeSqlDocumentSpy).toHaveBeenCalledOnce());

    fireEvent.click(screen.getByRole("button", { name: "rows_b" }));
    await waitFor(() => expect(sql).toHaveValue("SELECT 1"));
    fireEvent.input(sql, { target: { value: editedSql } });
    await waitFor(() => expect(sql).toHaveValue(editedSql));
    await waitFor(() =>
      expect(screen.getByRole("combobox", {
        name: "Choose a variable for month",
      })).toHaveValue("month")
    );

    resolveNormalize({
      source: editedSql,
      sql: "SELECT $month + 1",
      variables: [{ type: "string", name: "month", description: "" }],
      document: { status: "parsed" },
      replacements: [],
    });
    await Promise.resolve();
    expect(screen.getByRole("combobox", {
      name: "Choose a variable for month",
    })).toHaveValue("month");
    expect(draft().variables.find((node) => node.id === "rows_b"))
      .toMatchObject({ query: { arguments: [{ name: "month", variable: "month" }] } });

    fireEvent.click(screen.getByRole("button", { name: "rows_a" }));
    await waitFor(() => expect(sql).toHaveValue(editedSql));
    expect(screen.getByRole("combobox", {
      name: "Choose a variable for month",
    })).toHaveValue("");
    expect(draft().variables.find((node) => node.id === "rows_a"))
      .toMatchObject({ query: { arguments: [] } });
    expect(draft().variables.find((node) => node.id === "rows_b"))
      .toMatchObject({
        query: { arguments: [{ name: "month", variable: "month" }] },
      });
  });

  it("traps and restores focus when the mobile variable drawer closes", async () => {
    setLocale("en");
    const draft = createCompositionGraphDraft("Report");
    render(() => (
      <div id="app">
        <CompositionVariableGraphWorkspace
          draft={draft}
          selectedVariableId={null}
          onSelect={() => {}}
          onDraftChange={() => {}}
          onRemove={() => {}}
          sqlSchema={buildSqlSchema([])}
        />
      </div>
    ));

    const opener = screen.getByRole("button", { name: "Show variables" });
    fireEvent.click(opener);
    const dialog = screen.getByRole("dialog", { name: "Variables" });
    expect(dialog).toBeVisible();
    expect(document.getElementById("app")).toHaveAttribute("inert");
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Variables" })).toBeNull();
    expect(document.getElementById("app")).not.toHaveAttribute("inert");
    await waitFor(() => expect(opener).toHaveFocus());
  });

  it("moves from the mobile add menu into the name prompt", async () => {
    setLocale("en");
    const [draft, setDraft] = createSignal<CompositionGraphDraft>(
      createCompositionGraphDraft("Report"),
    );
    const [selected, setSelected] = createSignal<string | null>(null);
    const creation = creationProps(draft, setDraft, setSelected);
    render(() => (
      <div id="app">
        <CompositionVariableGraphWorkspace
          draft={draft()}
          selectedVariableId={selected()}
          onSelect={setSelected}
          onDraftChange={setDraft}
          pendingCreationKind={creation.pendingCreationKind()}
          onRequestCreation={creation.onRequestCreation}
          onCommitCreation={creation.onCommitCreation}
          onCancelCreation={creation.onCancelCreation}
          onRemove={() => {}}
          sqlSchema={buildSqlSchema([])}
        />
      </div>
    ));
    const opener = screen.getByRole("button", { name: "Show variables" });
    fireEvent.click(opener);
    const dialog = screen.getByRole("dialog", { name: "Variables" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add variable" }),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Input" }));

    expect(screen.queryByRole("dialog", { name: "Variables" }))
      .not.toBeInTheDocument();
    const name = screen.getByRole("textbox", { name: "Variable name" });
    await waitFor(() => expect(name).toHaveFocus());
    expect(document.getElementById("app")).not.toHaveAttribute("inert");
    fireEvent.keyDown(name, { key: "Escape" });
    expect(screen.queryByRole("textbox", { name: "Variable name" }))
      .not.toBeInTheDocument();
    expect(draft().variables).toHaveLength(0);
    await waitFor(() => expect(opener).toHaveFocus());
  });
});
