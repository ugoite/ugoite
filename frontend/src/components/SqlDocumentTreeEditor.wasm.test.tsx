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
  it("edits ARRAY elements entered directly as SQL through actual WASM", async () => {
    const initial = "SELECT ARRAY[4, 9]";
    const document = await parseSqlDocument(initial);
    const array = document.nodes.find((node) =>
      node.semantic?.kind === "array_literal"
    );
    expect(array).toBeDefined();
    expect(array?.kind).toBe("other_expression");
    const elements = document.nodes.filter((node) =>
      node.parent === array!.id && node.kind === "literal_expression"
    );
    expect(elements).toHaveLength(2);

    const [sql, setSql] = createSignal(initial);
    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const arrayItem = (await screen.findAllByRole("treeitem")).find((item) =>
      item.dataset.sqlDocumentNode === String(array!.id)
    );
    expect(arrayItem).toBeDefined();

    const secondElement = (await screen.findAllByRole("treeitem")).find(
      (item) => item.dataset.sqlDocumentNode === String(elements[1].id),
    );
    expect(secondElement).toBeDefined();
    fireEvent.click(secondElement!);
    expect(
      screen.getByRole("button", { name: "Move ARRAY element up" }),
    ).toBeEnabled();
    fireEvent.click(
      screen.getByRole("button", { name: "Move ARRAY element up" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(sql()).toBe("SELECT ARRAY[9, 4]"));
  });

  it("renames an output alias and its resolved bare ORDER BY reference atomically", async () => {
    const source = 'SELECT 1 AS "total" ORDER BY TOTAL DESC';
    const document = await parseSqlDocument(source);
    const item = document.nodes.find((node) => node.kind === "select_item");
    expect(item).toBeDefined();
    const [sql, setSql] = createSignal(source);
    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const treeItem = (await screen.findAllByRole("treeitem")).find((node) =>
      node.dataset.sqlDocumentNode === String(item!.id)
    );
    expect(treeItem).toBeDefined();
    fireEvent.click(treeItem!);
    const alias = screen.getByLabelText("Output name");
    fireEvent.input(alias, { target: { value: "renamed" } });
    fireEvent.blur(alias);
    await waitFor(() =>
      expect(sql()).toBe('SELECT 1 AS "renamed" ORDER BY "renamed" DESC')
    );
  });

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

  it("authors one-based array indexes and fixed-bound slices through actual WASM", async () => {
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
    fireEvent.change(screen.getByLabelText("Collection access"), {
      target: { value: "array_index" },
    });
    fireEvent.input(screen.getByLabelText("One-based index"), {
      target: { value: "0x10" },
    });
    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
    fireEvent.input(screen.getByLabelText("One-based index"), {
      target: { value: " 2" },
    });
    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
    fireEvent.input(screen.getByLabelText("One-based index"), {
      target: { value: "+2" },
    });
    expect((screen.getByLabelText("One-based index") as HTMLInputElement).value)
      .toBe("+2");
    expect(screen.getByRole("button", { name: "Apply" })).toBeEnabled();

    fireEvent.click(treeItem(amount!.id)!);
    await waitFor(() => {
      expect(
        (screen.getByLabelText("Collection access") as HTMLSelectElement)
          .value,
      ).toBe("");
      expect(screen.queryByLabelText("One-based index")).toBeNull();
    });

    fireEvent.click(treeItem(array!.id)!);
    fireEvent.change(screen.getByLabelText("Collection access"), {
      target: { value: "array_index" },
    });
    fireEvent.input(screen.getByLabelText("One-based index"), {
      target: { value: "+2" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(sql()).toBe("SELECT (ARRAY[4, 9])[2], amount /* preserve */")
    );
    const updated = await parseSqlDocument(sql());
    expect(updated.status).toBe("parsed");
    await waitFor(() => {
      expect(screen.queryByLabelText("Collection access")).toBeNull();
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
        (screen.getByLabelText("Collection access") as HTMLSelectElement)
          .value,
      ).toBe("");
      expect(screen.queryByLabelText("One-based index")).toBeNull();
    });

    const updatedArrayItem = updated.nodes.find((node) =>
      node.kind === "select_item" && node.range &&
      sql().slice(node.range.start, node.range.end) === "(ARRAY[4, 9])[2]"
    );
    expect(updatedArrayItem).toBeDefined();
    const updatedArrayTreeItem = (await screen.findAllByRole("treeitem")).find(
      (node) => node.dataset.sqlDocumentNode === String(updatedArrayItem!.id),
    );
    expect(updatedArrayTreeItem).toBeDefined();
    fireEvent.click(updatedArrayTreeItem!);
    fireEvent.change(screen.getByLabelText("Collection access"), {
      target: { value: "array_slice" },
    });
    fireEvent.input(screen.getByLabelText("One-based start"), {
      target: { value: " 1" },
    });
    fireEvent.input(screen.getByLabelText("One-based end"), {
      target: { value: "2" },
    });
    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
    fireEvent.input(screen.getByLabelText("One-based start"), {
      target: { value: "1" },
    });
    fireEvent.input(screen.getByLabelText("One-based end"), {
      target: { value: "0x10" },
    });
    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
    fireEvent.click(updatedTreeItem!);
    await waitFor(() => {
      expect(
        (screen.getByLabelText("Collection access") as HTMLSelectElement).value,
      ).toBe("");
      expect(screen.queryByLabelText("One-based start")).toBeNull();
      expect(screen.queryByLabelText("One-based end")).toBeNull();
    });
    fireEvent.click(updatedArrayTreeItem!);
    fireEvent.change(screen.getByLabelText("Collection access"), {
      target: { value: "array_slice" },
    });
    fireEvent.input(screen.getByLabelText("One-based start"), {
      target: { value: "1" },
    });
    fireEvent.input(screen.getByLabelText("One-based end"), {
      target: { value: "2" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(sql()).toBe(
        "SELECT ((ARRAY[4, 9])[2])[1:2], amount /* preserve */",
      )
    );
    view.unmount();
  });

  it("edits existing literal slice bounds in place through actual WASM", async () => {
    const initial =
      "SELECT values[ /* lower */ 1 /* after lower */ : /* upper */ 2 /* after upper */] AS chosen, amount FROM tasks WHERE amount > 0 ORDER BY amount";
    const document = await parseSqlDocument(initial);
    const slice = document.nodes.find((node) =>
      node.semantic?.kind === "array_slice"
    );
    expect(slice?.semantic).toMatchObject({
      kind: "array_slice",
      one_based_start: "1",
      one_based_end: "2",
    });
    const [sql, setSql] = createSignal(initial);
    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const sliceItem = (await screen.findAllByRole("treeitem")).find(
      (item) => item.dataset.sqlDocumentNode === String(slice!.id),
    );
    expect(sliceItem).toBeDefined();
    fireEvent.click(sliceItem!);
    expect(screen.getByLabelText("Collection access")).toHaveValue("array_slice");
    expect(screen.getByLabelText("One-based start")).toHaveValue("1");
    expect(screen.getByLabelText("One-based end")).toHaveValue("2");
    fireEvent.input(screen.getByLabelText("One-based start"), {
      target: { value: "3" },
    });
    fireEvent.input(screen.getByLabelText("One-based end"), {
      target: { value: "4" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(sql()).toBe(
        "SELECT values[ /* lower */ 3 /* after lower */ : /* upper */ 4 /* after upper */] AS chosen, amount FROM tasks WHERE amount > 0 ORDER BY amount",
      )
    );
  });

  it("adds a named Struct field through the actual WASM tree editor", async () => {
    const initial = "SELECT field_101[1] FROM tasks";
    const document = await parseSqlDocument(initial);
    const target = document.nodes.find((node) =>
      node.range !== null &&
      initial.slice(node.range.start, node.range.end) === "field_101[1]"
    );
    expect(target).toBeDefined();
    const [sql, setSql] = createSignal(initial);
    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const item = (await screen.findAllByRole("treeitem")).find(
      (candidate) => candidate.dataset.sqlDocumentNode === String(target!.id),
    );
    expect(item).toBeDefined();
    fireEvent.click(item!);
    fireEvent.change(screen.getByLabelText("Collection access"), {
      target: { value: "struct_field" },
    });
    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
    fireEvent.input(screen.getByLabelText("Field name"), {
      target: { value: "display'name" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(sql()).toBe("SELECT (field_101[1])['display''name'] FROM tasks")
    );
  });

  it("constructs and edits ARRAY elements through the expression inspector", async () => {
    const initial = "SELECT 4 /* preserve */";
    const [sql, setSql] = createSignal(initial);
    const view = render(() => (
      <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />
    ));
    const firstDocument = await parseSqlDocument(initial);
    const value = firstDocument.nodes.find((node) =>
      node.kind === "value" && node.range &&
      initial.slice(node.range.start, node.range.end) === "4"
    );
    expect(value).toBeDefined();
    const scalarProjection = firstDocument.nodes.find((node) =>
      node.kind === "select_item" && node.range &&
      initial.slice(node.range.start, node.range.end) === "4"
    );
    expect(scalarProjection).toBeDefined();
    const scalarProjectionItem = (await screen.findAllByRole("treeitem")).find(
      (node) => node.dataset.sqlDocumentNode === String(scalarProjection!.id),
    );
    expect(scalarProjectionItem).toBeDefined();
    fireEvent.click(scalarProjectionItem!);
    expect(
      (screen.getByLabelText("Expression value") as HTMLSelectElement)
        .querySelector('option[value="array"]'),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Remove selected ARRAY element" }),
    ).toBeNull();

    const firstItem = (await screen.findAllByRole("treeitem")).find((node) =>
      node.dataset.sqlDocumentNode === String(value!.id)
    );
    expect(firstItem).toBeDefined();
    fireEvent.click(firstItem!);
    fireEvent.change(screen.getByLabelText("Expression value"), {
      target: { value: "array" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(sql()).toBe("SELECT ARRAY[4] /* preserve */"));

    const arrayDocument = await parseSqlDocument(sql());
    const arrayExpression = arrayDocument.nodes.find((node) =>
      node.kind === "select_item" && node.range &&
      sql().slice(node.range.start, node.range.end) === "ARRAY[4]"
    );
    expect(arrayExpression).toBeDefined();
    expect(
      arrayDocument.nodes.some((node) =>
        node.parent === arrayExpression!.id &&
        node.semantic?.kind === "array_literal"
      ),
    ).toBe(true);
    const arrayExpressionItem = (await screen.findAllByRole("treeitem")).find(
      (node) => node.dataset.sqlDocumentNode === String(arrayExpression!.id),
    );
    expect(arrayExpressionItem).toBeDefined();
    fireEvent.click(arrayExpressionItem!);
    expect(
      (screen.getByLabelText("Expression value") as HTMLSelectElement)
        .querySelector('option[value="array"]'),
    ).not.toBeNull();
    fireEvent.change(screen.getByLabelText("Expression value"), {
      target: { value: "array" },
    });
    fireEvent.change(screen.getByLabelText("Value type"), {
      target: { value: "number" },
    });
    fireEvent.input(screen.getByLabelText("Number"), {
      target: { value: "9" },
    });
    expect(
      (screen.getByRole("button", { name: "Apply" }) as HTMLButtonElement)
        .disabled,
    ).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(sql()).toBe("SELECT ARRAY[4, 9] /* preserve */")
    );

    const initialArrayDocument = await parseSqlDocument(sql());
    const initialFirstArrayElement = initialArrayDocument.nodes.find((node) =>
      node.kind === "literal_expression" && node.range &&
      sql().slice(node.range.start, node.range.end) === "4"
    );
    expect(initialFirstArrayElement).toBeDefined();
    const initialFirstArrayElementItem =
      (await screen.findAllByRole("treeitem"))
        .find((node) =>
          node.dataset.sqlDocumentNode === String(initialFirstArrayElement!.id)
        );
    expect(initialFirstArrayElementItem).toBeDefined();
    fireEvent.click(initialFirstArrayElementItem!);
    const moveUpDisabled = screen.getByRole("button", {
      name: "Move ARRAY element up: Already the first ARRAY element",
    });
    expect(moveUpDisabled).toBeDisabled();
    expect(moveUpDisabled).toHaveAttribute(
      "title",
      "Already the first ARRAY element",
    );

    const lastArrayElement = initialArrayDocument.nodes.find((node) =>
      node.kind === "literal_expression" && node.range &&
      sql().slice(node.range.start, node.range.end) === "9"
    );
    expect(lastArrayElement).toBeDefined();
    const lastArrayElementItem = (await screen.findAllByRole("treeitem"))
      .find((node) =>
        node.dataset.sqlDocumentNode === String(lastArrayElement!.id)
      );
    expect(lastArrayElementItem).toBeDefined();
    fireEvent.click(lastArrayElementItem!);
    const moveDownDisabled = screen.getByRole("button", {
      name: "Move ARRAY element down: Already the last ARRAY element",
    });
    expect(moveDownDisabled).toBeDisabled();
    expect(moveDownDisabled).toHaveAttribute(
      "title",
      "Already the last ARRAY element",
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Move ARRAY element up" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(sql()).toBe("SELECT ARRAY[9, 4] /* preserve */")
    );

    const swappedArrayDocument = await parseSqlDocument(sql());
    const swappedFirstElement = swappedArrayDocument.nodes.find((node) =>
      node.kind === "literal_expression" && node.range &&
      sql().slice(node.range.start, node.range.end) === "9"
    );
    expect(swappedFirstElement).toBeDefined();
    const swappedFirstItem = (await screen.findAllByRole("treeitem"))
      .find((node) =>
        node.dataset.sqlDocumentNode === String(swappedFirstElement!.id)
      );
    expect(swappedFirstItem).toBeDefined();
    fireEvent.click(swappedFirstItem!);
    fireEvent.click(
      screen.getByRole("button", { name: "Move ARRAY element down" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(sql()).toBe("SELECT ARRAY[4, 9] /* preserve */")
    );

    const updatedDocument = await parseSqlDocument(sql());
    const firstArrayElement = updatedDocument.nodes.find((node) =>
      node.kind === "value" && node.range &&
      sql().slice(node.range.start, node.range.end) === "4"
    );
    expect(firstArrayElement).toBeDefined();
    const firstArrayElementItem = (await screen.findAllByRole("treeitem")).find(
      (node) => node.dataset.sqlDocumentNode === String(firstArrayElement!.id),
    );
    expect(firstArrayElementItem).toBeDefined();
    fireEvent.click(firstArrayElementItem!);
    fireEvent.change(screen.getByLabelText("Expression value"), {
      target: { value: "literal" },
    });
    fireEvent.change(screen.getByLabelText("Value type"), {
      target: { value: "number" },
    });
    fireEvent.input(screen.getByLabelText("Number"), {
      target: { value: "5" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(sql()).toBe("SELECT ARRAY[5, 9] /* preserve */")
    );
    const selectedFirstElementDocument = await parseSqlDocument(sql());
    const selectedFirstElement = selectedFirstElementDocument.nodes.find(
      (node) =>
        node.kind === "literal_expression" && node.range &&
        sql().slice(node.range.start, node.range.end) === "5",
    );
    expect(selectedFirstElement).toBeDefined();
    const selectedFirstElementItem = (await screen.findAllByRole("treeitem"))
      .find((node) =>
        node.dataset.sqlDocumentNode === String(selectedFirstElement!.id)
      );
    expect(selectedFirstElementItem).toBeDefined();
    fireEvent.click(selectedFirstElementItem!);
    fireEvent.click(
      screen.getByRole("button", { name: "Remove selected ARRAY element" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(sql()).toBe("SELECT ARRAY[9] /* preserve */"));

    const finalElementDocument = await parseSqlDocument(sql());
    const finalElement = finalElementDocument.nodes.find((node) =>
      node.kind === "literal_expression" && node.range &&
      sql().slice(node.range.start, node.range.end) === "9"
    );
    expect(finalElement).toBeDefined();
    const finalElementItem = (await screen.findAllByRole("treeitem")).find(
      (node) => node.dataset.sqlDocumentNode === String(finalElement!.id),
    );
    expect(finalElementItem).toBeDefined();
    fireEvent.click(finalElementItem!);
    fireEvent.click(
      screen.getByRole("button", { name: "Remove selected ARRAY element" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(sql()).toBe("SELECT ARRAY[] /* preserve */"));
    view.unmount();
  });

  it("hides ARRAY element removal when comments make the edit unsafe", async () => {
    const source = "SELECT ARRAY[4 /* keep inside */]";
    const document = await parseSqlDocument(source);
    const element = document.nodes.find((node) =>
      node.kind === "literal_expression" && node.range &&
      source.slice(node.range.start, node.range.end) === "4"
    );
    expect(element).toBeDefined();
    render(() => (
      <SqlDocumentTreeEditor sql={() => source} onSqlChange={() => {}} />
    ));
    const elementItem = (await screen.findAllByRole("treeitem")).find(
      (node) => node.dataset.sqlDocumentNode === String(element!.id),
    );
    expect(elementItem).toBeDefined();
    fireEvent.click(elementItem!);
    expect(
      screen.queryByRole("button", { name: "Remove selected ARRAY element" }),
    ).toBeNull();
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
    expect(screen.queryByLabelText("Collection access")).toBeNull();

    fireEvent.click(expressionControl!);
    fireEvent.change(screen.getByLabelText("Collection access"), {
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
