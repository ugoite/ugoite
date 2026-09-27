import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { PagedResultTable, type ResultColumn } from "./PagedResultTable";

interface Row {
  id: string;
  value: string;
}

const rows = (count: number): Row[] =>
  Array.from({ length: count }, (_, index) => ({
    id: `row-${index}`,
    value: `Value ${index}`,
  }));

const columns: ResultColumn<Row>[] = [{
  key: "value",
  label: "Value",
  cell: (row) => <button type="button">{row.value}</button>,
}];

const renderTable = (count: number) =>
  render(() => (
    <PagedResultTable
      columns={columns}
      rows={rows(count)}
      rowKey={(row) => row.id}
      pageIdentity="page-1"
      loading={false}
      loadingLabel="Loading"
      emptyLabel="No rows"
      retryLabel="Retry"
      canPrevious={false}
      canNext={false}
      previousLabel="Previous"
      nextLabel="Next"
      onPrevious={() => {}}
      onNext={() => {}}
      paginationLabel="Result pages"
    />
  ));

describe("PagedResultTable", () => {
  it.each([0, 1, 50])(
    "renders the %i-row page as a complete native table",
    (count) => {
      renderTable(count);
      if (count === 0) {
        expect(screen.getByText("No rows")).toBeInTheDocument();
        expect(screen.queryByRole("table")).not.toBeInTheDocument();
      } else {
        expect(screen.getAllByRole("row")).toHaveLength(count + 1);
      }
    },
  );

  it("renders the 100-row page as a complete native table", () => {
    renderTable(100);
    expect(screen.getByRole("table")).toBeInTheDocument();
    expect(screen.getAllByRole("row")).toHaveLength(101);
    expect(screen.getByRole("button", { name: "Value 99" }))
      .toBeInTheDocument();
  });

  it("shows a retry action for errors and suppresses the empty state", () => {
    const onRetry = vi.fn();
    render(() => (
      <PagedResultTable
        columns={columns}
        rows={[]}
        rowKey={(row) => row.id}
        pageIdentity="failed"
        loading={false}
        loadingLabel="Loading"
        error="Request failed"
        emptyLabel="No rows"
        retryLabel="Retry"
        onRetry={onRetry}
        canPrevious={false}
        canNext={false}
        previousLabel="Previous"
        nextLabel="Next"
        onPrevious={() => {}}
        onNext={() => {}}
        paginationLabel="Result pages"
      />
    ));
    expect(screen.getByRole("alert")).toHaveTextContent("Request failed");
    expect(screen.queryByText("No rows")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it("supports row selection and a native trailing action column", () => {
    const onRowSelect = vi.fn();
    const onAction = vi.fn();
    render(() => (
      <PagedResultTable
        columns={columns}
        rows={rows(1)}
        rowKey={(row) => row.id}
        pageIdentity="selectable"
        loading={false}
        loadingLabel="Loading"
        emptyLabel="No rows"
        retryLabel="Retry"
        canPrevious={false}
        canNext={false}
        previousLabel="Previous"
        nextLabel="Next"
        onPrevious={() => {}}
        onNext={() => {}}
        onRowSelect={onRowSelect}
        selectedRowKey="row-0"
        renderTrailingAction={() => (
          <button
            type="button"
            onClick={(event) => {
              event.stopPropagation();
              onAction();
            }}
          >
            Open row
          </button>
        )}
        trailingActionLabel="Actions"
        trailingActionClassName="sticky-action-cell"
        trailingHeaderClassName="sticky-action-header"
      />
    ));

    const row = screen.getByRole("button", { name: "Value 0" }).closest("tr");
    expect(row).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("columnheader", { name: "Actions" }))
      .toHaveClass("sticky-action-header");
    expect(row?.lastElementChild).toHaveClass("sticky-action-cell");

    fireEvent.click(row!);
    expect(onRowSelect).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Open row" }));
    expect(onAction).toHaveBeenCalledOnce();
    expect(onRowSelect).toHaveBeenCalledTimes(1);
  });

  it("moves keyboard focus between actions in ordinary rendered rows", async () => {
    render(() => (
      <PagedResultTable
        columns={[{ key: "value", label: "Value", cell: (row) => row.value }]}
        rows={rows(2)}
        rowKey={(row) => row.id}
        pageIdentity="keyboard"
        loading={false}
        loadingLabel="Loading"
        emptyLabel="No rows"
        retryLabel="Retry"
        canPrevious={false}
        canNext={false}
        previousLabel="Previous"
        nextLabel="Next"
        onPrevious={() => {}}
        onNext={() => {}}
        renderTrailingAction={(row) => (
          <button type="button">Open {row.id}</button>
        )}
        trailingActionLabel="Open"
      />
    ));

    const firstAction = screen.getByRole("button", { name: "Open row-0" });
    fireEvent.keyDown(firstAction, { key: "ArrowDown" });
    const secondAction = screen.getByRole("button", { name: "Open row-1" });
    await waitFor(() => expect(secondAction).toHaveFocus());
    fireEvent.keyDown(secondAction, { key: "ArrowUp" });
    await waitFor(() => expect(firstAction).toHaveFocus());
  });
});
