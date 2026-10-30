import "@testing-library/jest-dom/vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { describe, expect, it, vi } from "vitest";
import { CompositionVariableGraphWorkspace } from "./CompositionVariableGraphWorkspace";
import {
  type CompositionGraphDraft,
  type CompositionVariableNode,
  createCompositionGraphDraft,
} from "~/lib/composition-variable-binding";
import type { CompositionVariableCreationKind } from "~/components/CompositionVariableCreationForm";
import { buildSqlSchema } from "~/lib/sql";
import { setLocale } from "~/lib/i18n";

const { parseInputMock } = vi.hoisted(() => ({ parseInputMock: vi.fn() }));

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
  parseVariableGraphInput: (...args: unknown[]) => parseInputMock(...args),
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
    const [selected, setSelected] = createSignal<string | null>("input_1");
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

    expect(draft().variables[0]?.default).toBeUndefined();
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
    expect(draft().variables[0]?.default).toBeUndefined();
    expect(screen.getByRole("textbox", { name: "Default" })).toHaveValue("");
  });

  it("commits a variable rename while the ID field is edited", () => {
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
    fireEvent.input(screen.getByRole("textbox", { name: "Variable name" }), {
      target: { value: "period_start" },
    });

    expect(draft().variables[0]?.id).toBe("period_start");
    expect(selected()).toBe("period_start");
    expect(screen.getByRole("button", { name: "period_start" }))
      .toBeVisible();
  });

  it("binds declared SQL arguments only to compatible graph variables", () => {
    setLocale("en");
    const draft = createCompositionGraphDraft("Report");
    draft.variables = [
      {
        id: "threshold",
        kind: "constant",
        value_type: { scalar_type: "integer", nullable: false },
        value: { kind: "integer", value: "3" },
      },
      {
        id: "caption",
        kind: "constant",
        value_type: { scalar_type: "string", nullable: false },
        value: { kind: "string", value: "Orders" },
      },
      {
        id: "query_1",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: {
          kind: "inline_sql",
          sql: "SELECT $limit",
          arguments: [{ name: "limit", variable: "" }],
        },
      },
    ];
    const onDraftChange = vi.fn();
    render(() => (
      <CompositionVariableGraphWorkspace
        draft={draft}
        selectedVariableId="query_1"
        onSelect={() => {}}
        onDraftChange={onDraftChange}
        onRemove={() => {}}
        sqlSchema={buildSqlSchema([])}
        parameterDeclarations={{
          query_1: [{ name: "limit", type: "integer" }],
        }}
      />
    ));

    const binding = screen.getByRole("combobox", { name: "limit" });
    expect(binding).toHaveValue("");
    expect(within(binding).getByRole("option", { name: "threshold" }))
      .toBeInTheDocument();
    expect(within(binding).queryByRole("option", { name: "caption" }))
      .toBeNull();

    fireEvent.change(binding, { target: { value: "threshold" } });

    expect(onDraftChange).toHaveBeenCalledOnce();
    expect(
      onDraftChange.mock.calls[0][0].variables.find((node) =>
        node.id === "query_1"
      ),
    ).toMatchObject({
      query: {
        arguments: [{ name: "limit", variable: "threshold" }],
      },
    });
  });

  it("keeps SQL and argument bindings current through consecutive graph edits", async () => {
    setLocale("en");
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
            sql: "SELECT $minimum",
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
        parameterDeclarations={{ rows: [{ name: "minimum", type: "integer" }] }}
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
    const binding = screen.getByRole("combobox", { name: "minimum" });
    fireEvent.change(binding, { target: { value: "ceiling" } });
    await waitFor(() => expect(binding).toHaveValue("ceiling"));
    const editedSql = "SELECT $minimum + 1 AS amount";
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
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "SQL" }).textContent)
        .toContain(editedSql)
    );
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
