import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import { SqlResultTable } from "./SqlResultTable";

const renderTable = (columns: string[], rows: unknown[]) =>
  render(() => (
    <SqlResultTable
      columns={columns}
      rows={rows}
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
});
