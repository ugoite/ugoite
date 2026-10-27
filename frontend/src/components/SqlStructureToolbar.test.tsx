import "@testing-library/jest-dom/vitest";
import { createSignal, For } from "solid-js";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SqlDocument } from "~/lib/ugoite-client";
import type { SqlStructureAdapterContext } from "~/lib/sql";
import type { SQLWorkspaceAction } from "./SQLWorkspace";
import { SqlStructureToolbar } from "./SqlStructureToolbar";

const { applySemanticEditMock, parseDocumentMock } = vi.hoisted(() => ({
  applySemanticEditMock: vi.fn(),
  parseDocumentMock: vi.fn(),
}));

vi.mock("~/lib/ugoite-client", () => ({
  applySqlDocumentSemanticEditTransaction: applySemanticEditMock,
  parseSqlDocument: parseDocumentMock,
}));

const formContext: SqlStructureAdapterContext = {
  identity: "form-G",
  relationSelectable: true,
  label: "G",
  parser: {
    relation: "G",
    columns: [{ name: "amount", type: "integer" }],
    variables: [],
    stable_key: [],
  },
  columnCapabilities: {},
};

const makeSqlDocument = (source: string): SqlDocument => {
  const nodes: SqlDocument["nodes"] = [
    {
      id: 0,
      parent: null,
      kind: "query_statement",
      range: { start: 0, end: source.length },
      semantic: null,
    },
    {
      id: 1,
      parent: 0,
      kind: "query",
      range: { start: 0, end: source.length },
      semantic: null,
    },
    {
      id: 2,
      parent: 1,
      kind: "select",
      range: { start: 0, end: source.length },
      semantic: { kind: "select", distinct: false, distinct_on: false },
    },
    {
      id: 3,
      parent: 2,
      kind: "select_list",
      range: { start: source.indexOf("SELECT") + 7, end: source.length },
      semantic: null,
    },
    {
      id: 4,
      parent: 3,
      kind: "select_item",
      range: { start: source.indexOf("SELECT") + 7, end: source.length },
      semantic: null,
    },
    {
      id: 5,
      parent: 4,
      kind: "value",
      range: { start: source.indexOf("SELECT") + 7, end: source.length },
      semantic: { kind: "literal", literal_kind: "number", raw: "1" },
    },
  ];
  if (source.includes("UNION")) {
    nodes[2] = { ...nodes[2], parent: 6 };
    nodes.splice(2, 0, {
      id: 6,
      parent: 1,
      kind: "set_operation",
      range: { start: 0, end: source.length },
      semantic: {
        kind: "set_operation",
        operator: "UNION",
        quantifier: "DISTINCT",
      },
    });
  }
  if (source.startsWith("WITH")) {
    nodes.push({
      id: 7,
      parent: 1,
      kind: "with_clause",
      range: { start: 0, end: source.indexOf(" SELECT") },
      semantic: { kind: "with_clause", recursive: false, cte_count: 1 },
    });
    nodes.push({
      id: 8,
      parent: 7,
      kind: "common_table_expression",
      range: { start: 5, end: source.indexOf(")") + 1 },
      semantic: {
        kind: "common_table_expression",
        name: { value: "recent", quote_style: null },
        columns: [],
        materialized: null,
      },
    });
  }
  if (source.includes("FROM (")) {
    nodes.push({
      id: 7,
      parent: 2,
      kind: "from_clause",
      range: { start: source.indexOf("FROM"), end: source.length },
      semantic: null,
    });
    nodes.push({
      id: 8,
      parent: 7,
      kind: "relation_tree",
      range: {
        start: source.indexOf("(", source.indexOf("FROM")),
        end: source.length,
      },
      semantic: null,
    });
    nodes.push({
      id: 9,
      parent: 8,
      kind: "table_factor",
      range: {
        start: source.indexOf("(", source.indexOf("FROM")),
        end: source.length,
      },
      semantic: {
        kind: "table_factor",
        factor_kind: "derived",
        alias: null,
        column_aliases: [],
        has_column_alias_types: false,
      },
    });
  }
  return {
    contract_version: 22,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    conditions: [],
    nodes,
  };
};

function setup(
  sql: string,
  contexts: readonly SqlStructureAdapterContext[] = [],
) {
  const [actions, setActions] = createSignal<readonly SQLWorkspaceAction[]>([]);
  const [currentSql, setCurrentSql] = createSignal(sql);
  const [scope, setScope] = createSignal("space-a:query-1");
  render(() => (
    <>
      <div>
        <For each={actions()}>
          {(action) => (
            <button
              type="button"
              disabled={action.disabled}
              onClick={() => action.onActivate()}
            >
              {action.label}
            </button>
          )}
        </For>
      </div>
      <SqlStructureToolbar
        sql={currentSql}
        scope={scope}
        contexts={() => contexts}
        onSqlChange={setCurrentSql}
        onVariablesAdded={() => true}
        onActionsChange={setActions}
      />
    </>
  ));
  return { actions, currentSql, setScope };
}

describe("SqlStructureToolbar typed SQL Document actions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    parseDocumentMock.mockImplementation(async (source: string) =>
      makeSqlDocument(source)
    );
  });

  it.each([
    ["SELECT 1 UNION SELECT 2", "Set operation"],
    ["WITH recent AS (SELECT 1) SELECT 1", "Common table expression"],
    ["SELECT d.n FROM (SELECT 1 AS n) AS d", "Table source"],
  ])(
    "keeps Columns task-oriented and advanced edits reachable for admitted SQL %s",
    async (sql, expectedLabel) => {
      const { actions } = setup(sql);
      await waitFor(() => expect(actions()).toHaveLength(12));
      for (
        const label of [
          "Columns",
          "Filter",
          "Sort",
          "Aggregate",
          "Group by",
          "Distinct",
          "Row limit",
        ]
      ) {
        expect(actions().find((action) => action.label === label)?.disabled)
          .toBe(false);
      }
      fireEvent.click(screen.getByRole("button", { name: "Columns" }));
      await screen.findByRole("list", { name: "Result fields" });
      expect(screen.queryByRole("tree")).not.toBeInTheDocument();
      fireEvent.click(
        screen.getByRole("button", { name: "Advanced structure…" }),
      );
      const tree = await screen.findByRole("tree", { name: "SQL syntax tree" });
      expect(tree).toBeInTheDocument();
      expect(
        await screen.findByRole("treeitem", {
          name: new RegExp(`${expectedLabel}:`),
        }),
      ).toBeInTheDocument();
    },
  );

  it("adds Form G to SELECT 1 through the typed FROM operation", async () => {
    const initialSql = "SELECT 1";
    const changedSql = 'SELECT 1 FROM "G"';
    const { actions, currentSql } = setup(initialSql, [formContext]);
    applySemanticEditMock.mockResolvedValue({
      document: makeSqlDocument(changedSql),
      undo: { contract_version: 22, expected_source: changedSql, edits: [] },
    });

    await waitFor(() => expect(actions()).toHaveLength(12));
    fireEvent.click(screen.getByRole("button", { name: "Form" }));
    fireEvent.click(await screen.findByRole("button", { name: "G" }));

    await waitFor(() =>
      expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
        contract_version: 22,
        expected_source: initialSql,
        edits: [{
          kind: "add_from_relation",
          node_id: 2,
          expected_kind: "select",
          factor: {
            kind: "table",
            name: [{ value: "G", quote_style: '"' }],
            alias: null,
          },
        }],
      })
    );
    await waitFor(() => expect(currentSql()).toBe(changedSql));
  });

  it("keeps directly authored reserved relations editable and hides them from the Form chooser", async () => {
    const reserved = {
      ...formContext,
      identity: "reserved",
      relationSelectable: false,
      label: "Reserved",
      parser: { ...formContext.parser, relation: "Reserved" },
    };
    const source = 'SELECT 1 FROM "Reserved"';
    const { actions, currentSql } = setup(source, [formContext, reserved]);
    await waitFor(() => expect(actions()).toHaveLength(12));
    fireEvent.click(screen.getByRole("button", { name: "Form" }));
    expect(await screen.findByRole("button", { name: "G" }))
      .toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Reserved" })).not
      .toBeInTheDocument();
    expect(screen.queryByRole("tree")).not.toBeInTheDocument();
    expect(currentSql()).toBe(source);
    expect(applySemanticEditMock).not.toHaveBeenCalled();
  });

  it("closes successful Form selection and restores the toolbar focus owner", async () => {
    const { currentSql } = setup("SELECT 1", [formContext]);
    applySemanticEditMock.mockResolvedValue({
      document: makeSqlDocument('SELECT 1 FROM "G"'),
      undo: { expected_source: "", edits: [] },
    });
    const opener = await screen.findByRole("button", { name: "Form" });
    opener.focus();
    fireEvent.click(opener);
    const form = await screen.findByRole("button", { name: "G" });
    await waitFor(() => expect(form).toBeEnabled());
    fireEvent.click(form);
    await waitFor(() => expect(currentSql()).toBe('SELECT 1 FROM "G"'));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    await waitFor(() => expect(opener).toHaveFocus());
  });

  it("invalidates a pending Form edit across Spaces with identical query SQL and IDs", async () => {
    const { currentSql, setScope } = setup("SELECT 1", [formContext]);
    let finish!: (value: unknown) => void;
    applySemanticEditMock.mockImplementationOnce(() =>
      new Promise((resolve) => {
        finish = resolve;
      })
    );
    const opener = await screen.findByRole("button", { name: "Form" });
    fireEvent.click(opener);
    const form = await screen.findByRole("button", { name: "G" });
    await waitFor(() => expect(form).toBeEnabled());
    fireEvent.click(form);
    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    setScope("space-b:query-1");
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    finish({
      document: makeSqlDocument('SELECT 1 FROM "G"'),
      undo: { expected_source: "", edits: [] },
    });
    await Promise.resolve();
    expect(currentSql()).toBe("SELECT 1");
  });

  it("keeps Arguments available without Form context", async () => {
    const { actions } = setup("SELECT 1");
    await waitFor(() => expect(actions()).toHaveLength(12));
    const argumentsAction = actions().find((action) =>
      action.label === "Arguments"
    );
    expect(argumentsAction?.disabled).toBe(false);
    argumentsAction!.onActivate();
    expect(await screen.findByRole("dialog", { name: "Arguments" }))
      .toBeInTheDocument();
  });
});
