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

class ResizeObserverMock {
  constructor(private callback: ResizeObserverCallback) {}
  observe(target: Element) {
    this.callback([
      {
        target,
        contentRect: { height: 480 } as DOMRectReadOnly,
      } as ResizeObserverEntry,
    ], this as unknown as ResizeObserver);
  }
  unobserve() {}
  disconnect() {}
}

const renderTable = (count: number, identity = "page-1") =>
  render(() => (
    <PagedResultTable
      columns={columns}
      rows={rows(count)}
      rowKey={(row) => row.id}
      pageIdentity={identity}
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

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("PagedResultTable", () => {
  it.each([0, 1, 50, 100])(
    "keeps the %i-row page on the simple path",
    (count) => {
      renderTable(count);
      if (count === 0) {
        expect(screen.getByText("No rows")).toBeInTheDocument();
        expect(screen.queryByRole("table")).not.toBeInTheDocument();
      } else {
        expect(screen.getAllByRole("row")).toHaveLength(count + 1);
      }
      expect(document.querySelector(".paged-result-scroll-virtual"))
        .not.toBeInTheDocument();
    },
  );

  it("starts virtualizing at 101 rows", () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        return {
          x: 0,
          y: 0,
          top: 0,
          left: 0,
          right: 100,
          bottom: this.tagName === "TR" ? 48 : 480,
          width: 100,
          height: this.tagName === "TR" ? 48 : 480,
          toJSON: () => ({}),
        } as DOMRect;
      },
    );

    renderTable(101);

    const table = screen.getByRole("table");
    expect(table).toHaveAttribute("aria-rowcount", "102");
    expect(table.querySelectorAll("tbody tr[data-row-index]")).toHaveLength(22);
  });

  it("renders only viewport rows and exposes page-local table coordinates", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        return {
          x: 0,
          y: 0,
          top: 0,
          left: 0,
          right: 100,
          bottom: this.tagName === "TR" ? 48 : 480,
          width: 100,
          height: this.tagName === "TR" ? 48 : 480,
          toJSON: () => ({}),
        } as DOMRect;
      },
    );

    renderTable(1_000);

    const table = screen.getByRole("table");
    expect(table).toHaveAttribute("aria-rowcount", "1001");
    expect(within(table).getAllByRole("row")).toHaveLength(23);
    expect(table.querySelectorAll("tbody tr")).toHaveLength(23);
    expect(
      within(table).getAllByRole("row").filter((row) =>
        row.getAttribute("aria-hidden") !== "true"
      ),
    ).toHaveLength(23);
    expect(within(table).getAllByRole("row")[1]).toHaveAttribute(
      "aria-rowindex",
      "2",
    );

    const scroll = document.querySelector(
      ".paged-result-scroll-virtual",
    ) as HTMLDivElement;
    Object.defineProperty(scroll, "clientHeight", {
      configurable: true,
      value: 480,
    });
    scroll.scrollTop = 4_800;
    fireEvent.scroll(scroll);
    expect(within(table).getByText("Value 94")).toBeInTheDocument();
    expect(within(table).getByText("Value 115")).toBeInTheDocument();
    expect(within(table).queryByText("Value 0")).not.toBeInTheDocument();
    expect(within(table).getAllByRole("row").length).toBeLessThanOrEqual(30);
    expect(
      table.querySelectorAll(
        'tbody tr.paged-result-spacer[aria-hidden="true"]',
      ),
    ).toHaveLength(2);
    expect(within(table).getAllByRole("row")).toHaveLength(23);
    expect(within(table).getAllByRole("row")[1]).toHaveAttribute(
      "aria-rowindex",
      "96",
    );
  });

  it("moves keyboard focus to an offscreen row after rendering it", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        return {
          x: 0,
          y: 0,
          top: 0,
          left: 0,
          right: 100,
          bottom: this.tagName === "TR" ? 48 : 480,
          width: 100,
          height: this.tagName === "TR" ? 48 : 480,
          toJSON: () => ({}),
        } as DOMRect;
      },
    );
    renderTable(1_000);

    const scroll = document.querySelector(
      ".paged-result-scroll-virtual",
    ) as HTMLDivElement;
    Object.defineProperty(scroll, "clientHeight", {
      configurable: true,
      value: 480,
    });

    fireEvent.keyDown(screen.getByRole("button", { name: "Value 0" }), {
      key: "ArrowDown",
    });
    const nextVisible = await screen.findByRole("button", {
      name: "Value 1",
    });
    await waitFor(() => expect(nextVisible).toHaveFocus());

    const lastInitiallyVisible = screen.getByRole("button", {
      name: "Value 21",
    });
    fireEvent.keyDown(lastInitiallyVisible, { key: "ArrowDown" });
    const nextOffscreen = await screen.findByRole("button", {
      name: "Value 22",
    });
    await waitFor(() => expect(nextOffscreen).toHaveFocus());
  });

  it("falls back to ordinary rows when measured row height differs", () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        const height = this.tagName === "TR" ? 56 : 480;
        return {
          x: 0,
          y: 0,
          top: 0,
          left: 0,
          right: 100,
          bottom: height,
          width: 100,
          height,
          toJSON: () => ({}),
        } as DOMRect;
      },
    );
    renderTable(1_000);

    expect(document.querySelector(".paged-result-scroll-virtual"))
      .not.toBeInTheDocument();
    expect(screen.getAllByRole("row")).toHaveLength(1_001);
  });

  it("resets the scroll position when page identity changes", () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    const [identity, setIdentity] = createSignal("page-1");
    const [columnState, setColumnState] = createSignal(columns);
    render(() => (
      <PagedResultTable
        columns={columnState()}
        rows={rows(1_000)}
        rowKey={(row) => row.id}
        pageIdentity={identity()}
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
    const scroll = document.querySelector(
      ".paged-result-scroll-virtual",
    ) as HTMLDivElement;
    scroll.scrollTop = 1_200;
    fireEvent.scroll(scroll);
    expect(scroll.scrollTop).toBe(1_200);
    setColumnState([{ ...columns[0], key: "renamed", label: "Renamed" }]);
    expect(scroll.scrollTop).toBe(0);
    scroll.scrollTop = 1_200;
    fireEvent.scroll(scroll);
    setIdentity("page-2");
    expect(scroll.scrollTop).toBe(0);
  });

  it("shows failure instead of treating a failed empty page as empty", () => {
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
        onRetry={() => {}}
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
    expect(screen.getByRole("button", { name: "Retry" })).toBeEnabled();
  });

  it("supports selectable rows and a native trailing action column", () => {
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
    expect(screen.getByRole("table")).toBeInTheDocument();

    fireEvent.click(row!);
    expect(onRowSelect).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Open row" }));
    expect(onAction).toHaveBeenCalledTimes(1);
    expect(onRowSelect).toHaveBeenCalledTimes(1);
  });

  it("moves arrow-key focus to the trailing action when no primary action exists", async () => {
    const twoRows = rows(2);
    render(() => (
      <PagedResultTable
        columns={[{ key: "value", label: "Value", cell: (row) => row.value }]}
        rows={twoRows}
        rowKey={(row) => row.id}
        pageIdentity="trailing-keyboard"
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
