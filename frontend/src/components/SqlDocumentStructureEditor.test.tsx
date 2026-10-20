import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, within } from "@solidjs/testing-library";
import { afterEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import type { SqlDocument } from "~/lib/ugoite-client";
import { SqlDocumentStructureEditor } from "./SqlDocumentStructureEditor";

const source = `SELECT '猫' AS label FROM "G"`;
const byteLength = new TextEncoder().encode(source).byteLength;

const documentWith = (nodes: SqlDocument["nodes"]): SqlDocument => ({
  contract_version: 1,
  source,
  status: "parsed",
  diagnostics: [],
  tokens: [],
  nodes,
});

afterEach(() => setLocale("en"));

describe("SqlDocumentStructureEditor", () => {
  it("renders ranged nodes as a nested selectable list and omits un-ranged nodes", () => {
    setLocale("en");
    const document = documentWith([
      {
        id: 1,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: byteLength },
      },
      { id: 2, parent: 1, kind: "select", range: { start: 0, end: 21 } },
      { id: 3, parent: 2, kind: "select_item", range: { start: 7, end: 21 } },
      { id: 4, parent: 2, kind: "where_clause", range: null },
    ]);

    render(
      () => (
        <SqlDocumentStructureEditor
          document={document}
          onReplaceNode={vi.fn()}
        />
      ),
    );

    const navigation = screen.getByRole("navigation", {
      name: "SQL structure actions",
    });
    const lists = within(navigation).getAllByRole("list");
    expect(lists).toHaveLength(3);
    expect(within(navigation).getByRole("button", { name: "Columns" }))
      .toBeInTheDocument();
    expect(within(navigation).getByText("Columns")).toBeInTheDocument();
    expect(within(navigation).queryByRole("button", { name: "Filter" }))
      .not
      .toBeInTheDocument();

    for (const button of within(navigation).getAllByRole("button")) {
      expect(button).toHaveClass("ui-button");
      expect(button).toHaveStyle({ "min-height": "44px", "min-width": "44px" });
    }
  });

  it("uses UTF-8 byte ranges and emits the selected node replacement", () => {
    setLocale("en");
    const onReplaceNode = vi.fn();
    const document = documentWith([
      {
        id: 1,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: byteLength },
      },
      { id: 2, parent: 1, kind: "select_item", range: { start: 7, end: 21 } },
    ]);
    render(() => (
      <SqlDocumentStructureEditor
        document={document}
        onReplaceNode={onReplaceNode}
      />
    ));

    fireEvent.click(screen.getByRole("button", { name: "Columns" }));
    const fragment = screen.getByLabelText("Current SQL");
    expect(fragment.textContent).toBe("'猫' AS label");
    const replacement = screen.getByRole("textbox", {
      name: "Proposed SQL",
    });
    expect(replacement).toHaveValue("'猫' AS label");
    expect(replacement).toHaveStyle({
      "min-height": "44px",
      "min-width": "44px",
    });
    fireEvent.input(replacement, { target: { value: "COUNT(*) AS total" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply change" }));

    expect(onReplaceNode).toHaveBeenCalledOnce();
    expect(onReplaceNode).toHaveBeenCalledWith({
      node_id: 2,
      expected_kind: "select_item",
      replacement: "COUNT(*) AS total",
    });
  });

  it("rejects missing, invalid, and split-codepoint byte ranges", () => {
    setLocale("en");
    const onReplaceNode = vi.fn();
    const document = documentWith([
      { id: 1, parent: null, kind: "select_item", range: null },
      {
        id: 2,
        parent: null,
        kind: "select_item",
        range: { start: 3, end: byteLength + 1 },
      },
      // The cat is bytes 8..11; these endpoints split its three-byte sequence.
      {
        id: 3,
        parent: null,
        kind: "select_item",
        range: { start: 9, end: 10 },
      },
    ]);
    render(() => (
      <SqlDocumentStructureEditor
        document={document}
        onReplaceNode={onReplaceNode}
      />
    ));

    expect(screen.queryByRole("navigation", { name: "SQL structure actions" }))
      .not
      .toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Apply change" })).not
      .toBeInTheDocument();
    expect(onReplaceNode).not.toHaveBeenCalled();
  });
});
