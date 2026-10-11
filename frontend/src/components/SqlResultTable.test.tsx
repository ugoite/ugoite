import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import type { SqlResultColumnType } from "~/lib/types";
import { SqlResultTable } from "./SqlResultTable";

const renderTable = (
  columns: string[],
  rows: unknown[],
  columnTypes?: SqlResultColumnType[],
  exactIntegerRows?: (Array<string | null>)[],
) =>
  render(() => (
    <SqlResultTable
      columns={columns}
      columnTypes={columnTypes}
      rows={rows}
      exactIntegerRows={exactIntegerRows}
      pageIdentity="sql-page-1"
      tableLabel="SQL pages"
    />
  ));

describe("SqlResultTable", () => {
  it.each([0, 1, 50, 100])(
    "renders the %i-row page as a complete native table",
    (count) => {
      const rows = Array.from({ length: count }, (_, index) => [`row-${index}`]);
      renderTable(["value"], rows);
      if (count === 0) {
        expect(screen.queryByRole("table")).not.toBeInTheDocument();
      } else {
        expect(screen.getAllByRole("row")).toHaveLength(count + 1);
        expect(screen.getByText(`row-${count - 1}`)).toBeInTheDocument();
      }
    },
  );

  it("renders the 100-row page as a complete native table", () => {
    const rows = Array.from({ length: 100 }, (_, index) => [`row-${index}`]);
    renderTable(["value"], rows);
    expect(screen.getByRole("table")).toBeInTheDocument();
    expect(screen.getAllByRole("row")).toHaveLength(101);
    expect(screen.getByText("row-99")).toBeInTheDocument();
  });

  it("keeps exact column order for object rows", () => {
    renderTable(["b", "a"], [{ a: "first", b: "second" }]);
    const headers = screen.getAllByRole("columnheader");
    expect(headers.map((header) => header.textContent)).toEqual(["b", "a"]);
    const cells = within(screen.getAllByRole("row")[1]).getAllByRole("cell");
    expect(cells.map((cell) => cell.textContent)).toEqual(["second", "first"]);
  });

  it("keeps duplicate column names attached to their positions", () => {
    renderTable(["same", "same"], [["left", "right"]]);
    expect(screen.getAllByRole("columnheader", { name: "same" }))
      .toHaveLength(2);
    expect(screen.getByText("left")).toBeInTheDocument();
    expect(screen.getByText("right")).toBeInTheDocument();
  });

  it("formats scalar and structured values without selection semantics", () => {
    renderTable(
      ["text", "count", "active", "missing", "payload"],
      [[
        "hello",
        42,
        true,
        null,
        { ok: true },
      ]],
    );
    expect(screen.getByText("hello")).toBeInTheDocument();
    expect(screen.getByText("42")).toBeInTheDocument();
    expect(screen.getByText("true")).toBeInTheDocument();
    expect(screen.getAllByText("—").length).toBeGreaterThan(0);
    expect(screen.getByText('{"ok":true}')).toBeInTheDocument();
    expect(screen.queryByRole("navigation")).not.toBeInTheDocument();
    expect(
      document.querySelector("tr[aria-selected]"),
    ).toBeNull();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("fits narrow results without a fixed giant width", () => {
    const { container } = renderTable(["only"], [["one"]]);
    const table = container.querySelector("table.result-table--sql");
    expect(table).not.toBeNull();
    expect(table?.className).not.toMatch(/ui-table/);
    expect(
      container.querySelector(".result-table-viewport"),
    ).not.toBeNull();
  });

  it("names the table, exposes keyboard scroll, and marks numeric and long cells", () => {
    const longValue = "A long result value ".repeat(8);
    renderTable(
      ["amount", "description", "missing"],
      [[12.5, longValue, null]],
      ["float", "string", "string"],
    );

    const table = screen.getByRole("table", { name: "SQL pages" });
    const viewport = screen.getByRole("region", { name: "SQL pages" });
    expect(viewport).toHaveAttribute("tabindex", "0");
    expect(table).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: "amount" })).toHaveClass(
      "result-table-header--numeric",
    );
    expect(screen.getByRole("cell", { name: "12.5" })).toHaveClass(
      "result-table-cell--numeric",
    );
    expect(
      document.querySelector('td[data-column-key="description"] > span'),
    ).toHaveAttribute("title", longValue);
    expect(screen.getByRole("cell", { name: "—" })).toBeInTheDocument();
  });

  it("uses result schema types to align all-null numeric columns", () => {
    renderTable(["total"], [{ total: null }], ["float"]);

    expect(screen.getByRole("columnheader", { name: "total" })).toHaveClass(
      "result-table-header--numeric",
    );
    expect(screen.getByRole("cell", { name: "—" })).toHaveClass(
      "result-table-cell--numeric",
    );
  });

  it("keeps exact decimal strings numeric without converting them to JavaScript numbers", () => {
    renderTable(
      ["amount"],
      [["12345678901234567.8901"]],
      ["decimal"],
    );

    expect(screen.getByRole("columnheader", { name: "amount" })).toHaveClass(
      "result-table-header--numeric",
    );
    expect(
      screen.getByRole("cell", { name: "12345678901234567.8901" }),
    ).toHaveClass("result-table-cell--numeric");
  });

  it("prefers exact integer strings and falls back to legacy row numbers", () => {
    renderTable(
      ["wide", "ordinary", "missing"],
      [[9_223_372_036_854_776_000, 42, null]],
      ["integer", "integer", "integer"],
      [["9223372036854775807", "42", null]],
    );

    expect(screen.getByRole("cell", { name: "9223372036854775807" }))
      .toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "42" })).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "—" })).toBeInTheDocument();
  });

  it("falls back to existing integer row values when the sidecar is absent", () => {
    renderTable(["value"], [[42]], ["integer"]);

    expect(screen.getByRole("cell", { name: "42" })).toBeInTheDocument();
  });

  it("uses the server schema instead of inferring numeric rows", () => {
    renderTable(["payload"], [[42]], ["json"]);

    expect(screen.getByRole("columnheader", { name: "payload" }))
      .not.toHaveClass("result-table-header--numeric");
    expect(screen.getByRole("cell", { name: "42" })).not.toHaveClass(
      "result-table-cell--numeric",
    );
  });
});
