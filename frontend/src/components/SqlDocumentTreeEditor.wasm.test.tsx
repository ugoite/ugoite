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

  it("authors NOT and bitwise AND from the inspector and restores both with WASM undo", async () => {
    const initial = "SELECT - TRUE, 5 + 3";
    const original = await parseSqlDocument(initial);
    const unary = original.nodes.find((node) => node.kind === "unary_expression")!;
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
      node.kind === "binary_expression" && node.semantic?.kind === "binary_operator"
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
});
