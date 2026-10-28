import "@testing-library/jest-dom/vitest";
import { createSignal } from "solid-js";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import {
  applySqlDocumentEditTransaction,
  applySqlDocumentSemanticEditTransaction,
  parseSqlDocument,
  type SqlDocumentSemanticEditTransaction,
} from "~/lib/ugoite-client";
import { SqlDocumentTreeEditor } from "./SqlDocumentTreeEditor";

describe("SqlDocumentTreeEditor actual WASM interactions", () => {
  it("keeps composite ROLLUP and CUBE forms read-only without rewriting them", async () => {
    for (
      const [mode, initial] of [
        [
          "rollup",
          'SELECT a, b, c FROM "G" GROUP BY ROLLUP ((a, b), c)',
        ],
        [
          "cube",
          'SELECT a, b, c FROM "G" GROUP BY CUBE ((a, b), c)',
        ],
      ] as const
    ) {
      const document = await parseSqlDocument(initial);
      const select = document.nodes.find((node) => node.kind === "select")!;
      const group = document.nodes.find((node) =>
        node.kind === "group_by_clause"
      )!;
      expect(group.semantic).toMatchObject({
        kind: "group_by",
        mode,
        editable: false,
        groups: [[expect.any(Number), expect.any(Number)], [
          expect.any(Number),
        ]],
      });
      const [sql, setSql] = createSignal(initial);
      const view = render(() => (
        <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />
      ));
      const selectItem = (await screen.findAllByRole("treeitem")).find(
        (item) => item.dataset.sqlDocumentNode === String(select.id),
      );
      expect(selectItem).toBeDefined();
      fireEvent.click(selectItem!);
      expect(screen.getByLabelText("GROUP BY form")).toHaveValue(mode);
      expect(screen.getByText(/nested ROLLUP or CUBE structure is read-only/))
        .toBeVisible();
      expect(screen.getByRole("button", { name: "Apply GROUP BY form" }))
        .toBeDisabled();
      expect(screen.getByRole("button", { name: "Add group expression" }))
        .toBeDisabled();
      expect(sql()).toBe(initial);
      view.unmount();
    }
  });

  it("requires a typed condition before applying a semi join", async () => {
    const initial = 'SELECT g.id FROM "G" AS g WHERE g.id = 1';
    const document = await parseSqlDocument(initial);
    const relationTree = document.nodes.find((node) =>
      node.kind === "relation_tree"
    )!;
    const [sql, setSql] = createSignal(initial);
    render(() => (
      <SqlDocumentTreeEditor
        sql={sql}
        onSqlChange={setSql}
        relationOptions={() => [
          { relation: "G", label: "G" },
          { relation: "H", label: "H" },
        ]}
      />
    ));
    const relationItem = (await screen.findAllByRole("treeitem")).find(
      (item) => item.dataset.sqlDocumentNode === String(relationTree.id),
    );
    expect(relationItem).toBeDefined();
    fireEvent.click(relationItem!);
    fireEvent.change(screen.getByLabelText("Form"), {
      target: { value: "H" },
    });
    fireEvent.input(screen.getByLabelText("Alias"), {
      target: { value: "h" },
    });
    fireEvent.change(screen.getByLabelText("Join"), {
      target: { value: "left_semi" },
    });
    const constraint = screen.getByLabelText("Condition") as HTMLSelectElement;
    expect(constraint.querySelector('option[value="none"]')).toBeDisabled();
    const addJoin = screen.getByRole("button", { name: "Add join" });
    expect(addJoin).toBeDisabled();

    fireEvent.change(constraint, { target: { value: "on" } });
    const onExpression = screen.getByLabelText(
      "ON expression",
    ) as HTMLSelectElement;
    const conditionOption = Array.from(onExpression.options).find((option) =>
      option.textContent === "g.id = 1"
    );
    expect(conditionOption).toBeDefined();
    fireEvent.change(onExpression, {
      target: { value: conditionOption!.value },
    });
    expect(addJoin).toBeEnabled();
    fireEvent.click(addJoin);
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(sql()).toContain('LEFT SEMI JOIN "H" AS h ON g.id = 1')
    );
  });

  it("preserves grouping-set boundaries and authors the empty set from the inspector", async () => {
    const initial =
      'SELECT field_100, field_101 FROM "G" GROUP BY GROUPING SETS ((field_100, field_101), (field_100))';
    const document = await parseSqlDocument(initial);
    const select = document.nodes.find((node) => node.kind === "select")!;
    const groupBy = document.nodes.find((node) =>
      node.kind === "group_by_clause"
    )!;
    expect(groupBy.semantic).toMatchObject({
      kind: "group_by",
      mode: "grouping_sets",
      groups: expect.arrayContaining([
        expect.arrayContaining([expect.any(Number), expect.any(Number)]),
      ]),
    });
    const grouping = groupBy.semantic;
    if (grouping?.kind !== "group_by") {
      throw new Error("Missing group metadata");
    }
    expect(grouping.groups).toHaveLength(2);
    expect(grouping.groups[0]).toHaveLength(2);
    expect(grouping.groups[1]).toHaveLength(1);
    const expected = await applySqlDocumentSemanticEditTransaction(initial, {
      contract_version: document.contract_version,
      expected_source: initial,
      edits: [{
        kind: "set_group_by_structure",
        node_id: select.id,
        expected_kind: "select",
        mode: "grouping_sets",
        groups: [...grouping.groups, []],
      }],
    });
    const [sql, setSql] = createSignal(initial);
    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const selectItem = (await screen.findAllByRole("treeitem")).find(
      (item) => item.dataset.sqlDocumentNode === String(select.id),
    );
    expect(selectItem).toBeDefined();
    fireEvent.click(selectItem!);
    expect(screen.getByLabelText("GROUP BY form")).toHaveValue("grouping_sets");
    expect(screen.getByLabelText("Grouping set 1")).toHaveValue(
      grouping.groups[0].map(String),
    );
    expect(screen.getByLabelText("Grouping set 2")).toHaveValue(
      grouping.groups[1].map(String),
    );
    fireEvent.click(screen.getByRole("button", { name: "Add grouping set" }));
    fireEvent.click(
      screen.getByRole("button", { name: "Apply GROUP BY form" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(sql()).toBe(expected.document.source));
    expect(sql()).toContain(
      "GROUPING SETS ((field_100, field_101), (field_100), ())",
    );
  });

  it("authors ROLLUP with selected typed keys and restores it with WASM undo", async () => {
    const initial = 'SELECT field_100, field_101 FROM "G"';
    const document = await parseSqlDocument(initial);
    const select = document.nodes.find((node) => node.kind === "select")!;
    const keys = document.nodes.filter((node) =>
      node.kind === "identifier_expression" &&
      node.range && ["field_100", "field_101"].includes(
        document.source.slice(node.range.start, node.range.end),
      )
    );
    const expected = await applySqlDocumentSemanticEditTransaction(initial, {
      contract_version: document.contract_version,
      expected_source: initial,
      edits: [{
        kind: "set_group_by_structure",
        node_id: select.id,
        expected_kind: "select",
        mode: "rollup",
        groups: [[keys[0].id, keys[1].id]],
      }],
    });
    const [sql, setSql] = createSignal(initial);
    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const selectItem = (await screen.findAllByRole("treeitem")).find(
      (item) => item.dataset.sqlDocumentNode === String(select.id),
    );
    expect(selectItem).toBeDefined();
    fireEvent.click(selectItem!);
    fireEvent.change(screen.getByLabelText("GROUP BY form"), {
      target: { value: "rollup" },
    });
    const keySelect = screen.getByLabelText(
      "Grouping keys",
    ) as HTMLSelectElement;
    for (const option of Array.from(keySelect.options)) {
      option.selected = keys.some((key) => String(key.id) === option.value);
    }
    fireEvent.change(keySelect);
    fireEvent.click(
      screen.getByRole("button", { name: "Apply GROUP BY form" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(sql()).toBe(expected.document.source));
    expect(sql()).toContain("GROUP BY ROLLUP (field_100, field_101)");
    const restored = await applySqlDocumentEditTransaction(
      sql(),
      expected.undo,
    );
    expect(restored.document.source).toBe(initial);
  });

  it("adds a typed aggregate FILTER from the inspector and restores it with WASM undo", async () => {
    const initial = "SELECT COUNT(amount)";
    const document = await parseSqlDocument(initial);
    const aggregate = document.nodes.find((node) =>
      node.kind === "function_expression" && node.semantic?.kind === "function"
    )!;
    const expected = await applySqlDocumentSemanticEditTransaction(initial, {
      contract_version: document.contract_version,
      expected_source: initial,
      edits: [{
        kind: "set_aggregate_filter",
        node_id: aggregate.id,
        expected_kind: "function_expression",
        enabled: true,
      }],
    });
    const [sql, setSql] = createSignal(initial);
    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const aggregateItem = (await screen.findAllByRole("treeitem")).find(
      (item) => item.dataset.sqlDocumentNode === String(aggregate.id),
    );
    expect(aggregateItem).toBeDefined();
    fireEvent.click(aggregateItem!);
    fireEvent.click(screen.getByLabelText("Aggregate FILTER"));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(sql()).toBe(expected.document.source));
    expect(sql()).toContain("FILTER (WHERE TRUE)");
    const restored = await applySqlDocumentEditTransaction(
      sql(),
      expected.undo,
    );
    expect(restored.document.source).toBe(initial);
  });

  it("authors one-based array access through actual WASM", async () => {
    const source = "SELECT ARRAY[4, 9], amount /* preserve */";
    const document = await parseSqlDocument(source);
    const array = document.nodes.find((node) =>
      node.range && source.slice(node.range.start, node.range.end) ===
        "ARRAY[4, 9]"
    );
    const amount = document.nodes.find((node) =>
      node.kind === "identifier_expression" && node.range &&
      source.slice(node.range.start, node.range.end) === "amount"
    );
    expect(array).toBeDefined();
    expect(amount).toBeDefined();
    const [sql, setSql] = createSignal(source);
    const view = render(() => (
      <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />
    ));
    const items = await screen.findAllByRole("treeitem");
    const treeItem = (id: number) =>
      items.find((node) => node.dataset.sqlDocumentNode === String(id));
    expect(treeItem(array!.id)).toBeDefined();
    expect(treeItem(amount!.id)).toBeDefined();

    fireEvent.click(treeItem(array!.id)!);
    fireEvent.change(screen.getByLabelText("Add array element index"), {
      target: { value: "array_index" },
    });
    fireEvent.input(screen.getByLabelText("One-based index"), {
      target: { value: "2" },
    });
    expect((screen.getByLabelText("One-based index") as HTMLInputElement).value)
      .toBe("2");

    fireEvent.click(treeItem(amount!.id)!);
    await waitFor(() => {
      expect(
        (screen.getByLabelText("Add array element index") as HTMLSelectElement)
          .value,
      ).toBe("");
      expect(screen.queryByLabelText("One-based index")).toBeNull();
    });

    fireEvent.click(treeItem(array!.id)!);
    fireEvent.change(screen.getByLabelText("Add array element index"), {
      target: { value: "array_index" },
    });
    fireEvent.input(screen.getByLabelText("One-based index"), {
      target: { value: "2" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(sql()).toBe("SELECT (ARRAY[4, 9])[2], amount /* preserve */")
    );
    const updated = await parseSqlDocument(sql());
    expect(updated.status).toBe("parsed");
    await waitFor(() => {
      expect(screen.queryByLabelText("Add array element index")).toBeNull();
      expect(screen.queryByLabelText("One-based index")).toBeNull();
    });
    const updatedAmount = updated.nodes.find((node) =>
      node.kind === "identifier_expression" && node.range &&
      sql().slice(node.range.start, node.range.end) === "amount"
    );
    expect(updatedAmount).toBeDefined();
    const updatedTreeItem = (await screen.findAllByRole("treeitem")).find(
      (node) => node.dataset.sqlDocumentNode === String(updatedAmount!.id),
    );
    expect(updatedTreeItem).toBeDefined();
    fireEvent.click(updatedTreeItem!);
    await waitFor(() => {
      expect(
        (screen.getByLabelText("Add array element index") as HTMLSelectElement)
          .value,
      ).toBe("");
      expect(screen.queryByLabelText("One-based index")).toBeNull();
    });
    view.unmount();
  });

  it("indexes an aliased projection child without changing its alias", async () => {
    const source = "SELECT field_101 AS values_out";
    const document = await parseSqlDocument(source);
    const item = document.nodes.find((node) =>
      node.kind === "select_item" && node.semantic?.kind === "projection_alias"
    );
    const expression = document.nodes.find((node) =>
      node.kind === "identifier_expression" && node.range &&
      source.slice(node.range.start, node.range.end) === "field_101"
    );
    expect(item).toBeDefined();
    expect(expression).toBeDefined();

    const [sql, setSql] = createSignal(source);
    const view = render(() => (
      <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />
    ));
    const items = await screen.findAllByRole("treeitem");
    const itemControl = items.find((node) =>
      node.dataset.sqlDocumentNode === String(item!.id)
    );
    const expressionControl = items.find((node) =>
      node.dataset.sqlDocumentNode === String(expression!.id)
    );
    expect(itemControl).toBeDefined();
    expect(expressionControl).toBeDefined();
    fireEvent.click(itemControl!);
    expect(screen.queryByLabelText("Add array element index")).toBeNull();

    fireEvent.click(expressionControl!);
    fireEvent.change(screen.getByLabelText("Add array element index"), {
      target: { value: "array_index" },
    });
    fireEvent.input(screen.getByLabelText("One-based index"), {
      target: { value: "1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(sql()).toBe("SELECT (field_101)[1] AS values_out")
    );
    view.unmount();
  });

  it("switches CASE to a selected operand and back through the CASE mode control", async () => {
    const initial = "SELECT amount, CASE WHEN amount = 1 THEN 2 ELSE 3 END";
    const [sql, setSql] = createSignal(initial);
    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);

    fireEvent.click(
      await screen.findByRole("treeitem", { name: /CASE: CASE WHEN amount/ }),
    );
    const mode = screen.getByLabelText("CASE operand") as HTMLSelectElement;
    const amountOption = Array.from(mode.options).find((option) =>
      option.value !== "" && option.textContent?.trim() === "amount"
    );
    expect(amountOption).toBeDefined();
    fireEvent.change(mode, { target: { value: amountOption!.value } });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(sql()).toContain("CASE amount WHEN"));

    fireEvent.click(
      await screen.findByRole("treeitem", { name: /CASE: CASE amount WHEN/ }),
    );
    fireEvent.change(screen.getByLabelText("CASE operand"), {
      target: { value: "" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(sql()).toContain("CASE  WHEN amount = 1"));
  });

  it("edits an existing NOT expression to a unary sign and restores it with WASM undo", async () => {
    const initial = "SELECT NOT TRUE";
    const document = await parseSqlDocument(initial);
    const unary = document.nodes.find((node) =>
      node.kind === "unary_expression"
    )!;
    const edit: SqlDocumentSemanticEditTransaction = {
      contract_version: document.contract_version,
      expected_source: initial,
      edits: [{
        kind: "replace_unary_sign",
        node_id: unary.id,
        expected_kind: "unary_expression",
        sign: "negative",
      }],
    };
    const result = await applySqlDocumentSemanticEditTransaction(initial, edit);
    const [sql, setSql] = createSignal(initial);
    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Unary expression/ }),
    );
    fireEvent.change(screen.getByLabelText("Unary sign"), {
      target: { value: "negative" },
    });
    expect(screen.getByRole("button", { name: "Apply" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(sql()).toBe(result.document.source));
    expect(sql()).toContain("- TRUE");

    const restored = await applySqlDocumentEditTransaction(sql(), result.undo);
    expect(restored.document.source).toBe(initial);
  });

  it("authors NOT and bitwise AND from the inspector and restores both with WASM undo", async () => {
    const initial = "SELECT - TRUE, 5 + 3";
    const original = await parseSqlDocument(initial);
    const unary = original.nodes.find((node) =>
      node.kind === "unary_expression"
    )!;
    const unaryEdit: SqlDocumentSemanticEditTransaction = {
      contract_version: original.contract_version,
      expected_source: initial,
      edits: [{
        kind: "replace_unary_sign",
        node_id: unary.id,
        expected_kind: "unary_expression",
        sign: "not",
      }],
    };
    const unaryResult = await applySqlDocumentSemanticEditTransaction(
      initial,
      unaryEdit,
    );
    const afterNot = unaryResult.document.source;
    const afterNotDocument = await parseSqlDocument(afterNot);
    const binary = afterNotDocument.nodes.find((node) =>
      node.kind === "binary_expression" &&
      node.semantic?.kind === "binary_operator"
    )!;
    const bitwiseEdit: SqlDocumentSemanticEditTransaction = {
      contract_version: afterNotDocument.contract_version,
      expected_source: afterNot,
      edits: [{
        kind: "replace_binary_operator",
        node_id: binary.id,
        expected_kind: "binary_expression",
        operator: "bitwise_and",
      }],
    };
    const bitwiseResult = await applySqlDocumentSemanticEditTransaction(
      afterNot,
      bitwiseEdit,
    );

    const [sql, setSql] = createSignal(initial);
    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Unary expression/ }),
    );
    fireEvent.change(screen.getByLabelText("Unary sign"), {
      target: { value: "not" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(sql()).toBe(afterNot));

    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Binary expression/ }),
    );
    fireEvent.change(screen.getByLabelText("Operator"), {
      target: { value: "bitwise_and" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(sql()).toBe(bitwiseResult.document.source));
    expect(sql()).toContain("NOT TRUE");
    expect(sql()).toContain("5 & 3");

    const restoredBinary = await applySqlDocumentEditTransaction(
      sql(),
      bitwiseResult.undo,
    );
    const restoredUnary = await applySqlDocumentEditTransaction(
      restoredBinary.document.source,
      unaryResult.undo,
    );
    expect(restoredUnary.document.source).toBe(initial);
  });

  it("authors typed DATE literals from the inspector and restores them with WASM undo", async () => {
    const initial = "SELECT amount";
    const document = await parseSqlDocument(initial);
    const expression = document.nodes.find((node) =>
      node.kind === "identifier_expression"
    )!;
    const result = await applySqlDocumentSemanticEditTransaction(initial, {
      contract_version: document.contract_version,
      expected_source: initial,
      edits: [{
        kind: "replace_literal",
        node_id: expression.id,
        expected_kind: expression.kind,
        literal: {
          kind: "typed_string",
          data_type: "date",
          value: "2026-02-03",
        },
      }],
    });
    const [sql, setSql] = createSignal(initial);
    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Identifier: amount/ }),
    );
    fireEvent.change(screen.getByLabelText("Expression value"), {
      target: { value: "literal" },
    });
    fireEvent.change(screen.getByLabelText("Value type"), {
      target: { value: "date" },
    });
    fireEvent.input(screen.getByLabelText("DATE value"), {
      target: { value: "2026-02-03" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(sql()).toBe(result.document.source));
    expect(sql()).toBe("SELECT DATE '2026-02-03'");
    const restored = await applySqlDocumentEditTransaction(sql(), result.undo);
    expect(restored.document.source).toBe(initial);
  });

  it("constructs a NULL-safe predicate from the inspector and restores it with WASM undo", async () => {
    const initial = "SELECT amount";
    const document = await parseSqlDocument(initial);
    const expression = document.nodes.find((node) =>
      node.kind === "identifier_expression"
    )!;
    const result = await applySqlDocumentSemanticEditTransaction(initial, {
      contract_version: document.contract_version,
      expected_source: initial,
      edits: [{
        kind: "wrap_expression_in_predicate",
        node_id: expression.id,
        expected_kind: expression.kind,
        template: "is_not_distinct_from",
      }],
    });
    const [sql, setSql] = createSignal(initial);
    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Identifier: amount/ }),
    );
    fireEvent.change(screen.getByLabelText("Predicate"), {
      target: { value: "is_not_distinct_from" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(sql()).toBe(result.document.source));
    expect(sql()).toBe("SELECT (amount) IS NOT DISTINCT FROM NULL");
    const restored = await applySqlDocumentEditTransaction(sql(), result.undo);
    expect(restored.document.source).toBe(initial);
  });

  it("constructs a parameter LIMIT through the actual WASM tree editor", async () => {
    const initial = "SELECT field_100 FROM tasks";
    const [sql, setSql] = createSignal(initial);
    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);

    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Query: SELECT field_100/ }),
    );
    fireEvent.input(screen.getByLabelText("LIMIT parameter name"), {
      target: { value: "page_size" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Set LIMIT parameter" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(sql()).toBe("SELECT field_100 FROM tasks LIMIT $page_size")
    );
  });

  it("renames a CTE output column and its binding through the actual WASM tree editor", async () => {
    const initial = "WITH c (old_name) AS (SELECT 1) SELECT c.old_name FROM c";
    const [sql, setSql] = createSignal(initial);
    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);

    fireEvent.click(
      await screen.findByRole("treeitem", {
        name: /Common table expression: c \(old_name\)/,
      }),
    );
    fireEvent.input(
      screen.getByLabelText("Output column aliases (comma-separated)"),
      {
        target: { value: "new_name" },
      },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Set output column aliases" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(sql()).toBe(
        "WITH c (new_name) AS (SELECT 1) SELECT c.new_name FROM c",
      )
    );
  });
});
