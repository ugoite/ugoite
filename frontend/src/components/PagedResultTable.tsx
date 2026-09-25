import {
  type Accessor,
  createEffect,
  createMemo,
  createSignal,
  For,
  type JSX,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import { LocalBusyIndicator } from "./LocalBusyIndicator";
import {
  calculateVisibleRowRange,
  VIRTUAL_ROW_HEIGHT,
  VIRTUAL_ROW_THRESHOLD,
} from "~/lib/virtual-rows";

export interface ResultColumn<Row> {
  key: string;
  label: string;
  cell: (row: Row, rowIndex: number) => JSX.Element;
}

export interface PagedResultTableProps<Row> {
  columns: readonly ResultColumn<Row>[];
  rows: readonly Row[];
  rowKey: (row: Row, index: number) => string;
  pageIdentity: string;
  loading: boolean;
  loadingLabel: string;
  error?: string | null;
  emptyLabel: string;
  retryLabel: string;
  onRetry?: () => void;
  canPrevious: boolean;
  canNext: boolean;
  previousLabel: string;
  nextLabel: string;
  onPrevious: () => void;
  onNext: () => void;
  renderPrimaryAction?: (row: Row, index: number) => JSX.Element;
  renderTrailingAction?: (row: Row, index: number) => JSX.Element;
  trailingActionLabel?: string;
  entryDataId?: (row: Row) => string;
  classNames?: { table?: string; scroll?: string };
  paginationLabel?: string;
}

export function PagedResultTable<Row>(props: PagedResultTableProps<Row>) {
  let scrollElement: HTMLDivElement | undefined;
  const [scrollTop, setScrollTop] = createSignal(0);
  const [viewportHeight, setViewportHeight] = createSignal(480);
  const [supportsResizeObserver, setSupportsResizeObserver] = createSignal(
    true,
  );
  const [measuredRowHeight, setMeasuredRowHeight] = createSignal<number>();

  const virtualized = createMemo(() =>
    props.rows.length > VIRTUAL_ROW_THRESHOLD && supportsResizeObserver() &&
    (!measuredRowHeight() ||
      Math.abs(measuredRowHeight()! - VIRTUAL_ROW_HEIGHT) < 1)
  );
  const range = createMemo(() =>
    virtualized()
      ? calculateVisibleRowRange(
        props.rows.length,
        scrollTop(),
        viewportHeight(),
      )
      : {
        start: 0,
        end: props.rows.length,
        topSpacerHeight: 0,
        bottomSpacerHeight: 0,
      }
  );
  const visibleRows = createMemo(() => {
    const { start, end } = range();
    return props.rows.slice(start, end);
  });
  const identity = () =>
    `${props.pageIdentity}\u0000${
      props.columns.map((column) => column.key)
        .join("\u0000")
    }`;

  createEffect(() => {
    identity();
    setScrollTop(0);
    if (scrollElement) scrollElement.scrollTop = 0;
  });

  onMount(() => {
    if (!scrollElement) return;
    if (typeof ResizeObserver === "undefined") {
      setSupportsResizeObserver(false);
      return;
    }
    const observer = new ResizeObserver((entries) => {
      const height = entries[0]?.contentRect.height ??
        scrollElement?.clientHeight;
      if (height && height > 0) setViewportHeight(height);
      const firstRow = scrollElement?.querySelector<HTMLElement>(
        "tbody tr[data-row-index]",
      );
      if (firstRow) {
        const rowHeight = firstRow.getBoundingClientRect().height;
        if (rowHeight > 0) setMeasuredRowHeight(rowHeight);
      }
    });
    observer.observe(scrollElement);
    onCleanup(() => observer.disconnect());
  });

  const handleScroll: JSX.EventHandlerUnion<HTMLDivElement, Event> = (
    event,
  ) => {
    setScrollTop(event.currentTarget.scrollTop);
  };

  const focusRow = (index: number) => {
    if (index < 0 || index >= props.rows.length) return;
    if (scrollElement && virtualized()) {
      const rowTop = (index + 1) * VIRTUAL_ROW_HEIGHT;
      const rowBottom = rowTop + VIRTUAL_ROW_HEIGHT;
      if (rowTop < scrollElement.scrollTop) scrollElement.scrollTop = rowTop;
      else if (
        rowBottom > scrollElement.scrollTop + scrollElement.clientHeight
      ) {
        scrollElement.scrollTop = rowBottom - scrollElement.clientHeight;
      }
      setScrollTop(scrollElement.scrollTop);
    }
    const focusTarget = () => {
      scrollElement?.querySelector<HTMLElement>(
        `tr[data-row-index="${index}"] button`,
      )?.focus();
    };
    if (typeof requestAnimationFrame === "function") {
      requestAnimationFrame(focusTarget);
    } else {
      setTimeout(focusTarget, 0);
    }
  };

  const handleTableKeyDown: JSX.EventHandlerUnion<
    HTMLElement,
    KeyboardEvent
  > = (event) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const target = event.target;
    if (!(target instanceof HTMLElement)) return;
    const row = target.closest<HTMLElement>("tr[data-row-index]");
    if (!row || !row.querySelector("button")) return;
    const index = Number(row.dataset.rowIndex);
    if (!Number.isInteger(index)) return;
    event.preventDefault();
    focusRow(index + (event.key === "ArrowDown" ? 1 : -1));
  };

  const renderIndexedRow = (row: Row, index: Accessor<number>) => (
    <tr
      class="paged-result-row"
      data-row-index={index()}
      data-row-key={props.rowKey(row, index())}
      data-entry-id={props.entryDataId?.(row)}
      aria-rowindex={virtualized() ? index() + 2 : undefined}
      onKeyDown={handleTableKeyDown}
    >
      <For each={props.columns}>
        {(column, columnIndex) => (
          <td class="paged-result-cell" data-column-key={column.key}>
            <Show
              when={columnIndex() === 0 && props.renderPrimaryAction}
              fallback={column.cell(row, index())}
            >
              {props.renderPrimaryAction!(row, index())}
            </Show>
          </td>
        )}
      </For>
      <Show when={props.renderTrailingAction}>
        <td class="paged-result-cell">
          {props.renderTrailingAction!(row, index())}
        </td>
      </Show>
    </tr>
  );

  return (
    <section class="paged-result-table" aria-busy={props.loading || undefined}>
      <Show when={props.loading}>
        <LocalBusyIndicator label={props.loadingLabel} />
      </Show>
      <Show when={props.error}>
        <p class="ui-text-danger" role="alert">{props.error}</p>
        <Show when={props.onRetry}>
          <button
            type="button"
            class="ui-button ui-button-secondary"
            disabled={props.loading}
            onClick={() =>
              props.onRetry?.()}
          >
            {props.retryLabel}
          </button>
        </Show>
      </Show>
      <Show when={!props.loading && !props.error && props.rows.length === 0}>
        <p class="ui-muted">{props.emptyLabel}</p>
      </Show>
      <Show when={props.rows.length > 0 && !props.error}>
        <div
          ref={scrollElement}
          class={props.classNames?.scroll ?? "paged-result-scroll"}
          classList={{ "paged-result-scroll-virtual": virtualized() }}
          tabIndex={virtualized() && !props.renderPrimaryAction ? 0 : undefined}
          aria-label={props.paginationLabel}
          onScroll={handleScroll}
        >
          <table
            class={props.classNames?.table ?? "paged-result-table-element"}
            aria-rowcount={virtualized() ? props.rows.length + 1 : undefined}
          >
            <thead>
              <tr aria-rowindex={virtualized() ? 1 : undefined}>
                <For each={props.columns}>
                  {(column) => <th scope="col">{column.label}</th>}
                </For>
                <Show when={props.renderTrailingAction}>
                  <th scope="col">
                    <span class="ui-sr-only">
                      {props.trailingActionLabel}
                    </span>
                  </th>
                </Show>
              </tr>
            </thead>
            <tbody>
              <Show when={virtualized() && range().topSpacerHeight > 0}>
                <tr aria-hidden="true" class="paged-result-spacer">
                  <td
                    colspan={props.columns.length +
                      (props.renderTrailingAction ? 1 : 0)}
                    style={{ height: `${range().topSpacerHeight}px` }}
                  />
                </tr>
              </Show>
              <For each={visibleRows()}>
                {(row, visibleIndex) => {
                  const index = () => range().start + visibleIndex();
                  return renderIndexedRow(row, index);
                }}
              </For>
              <Show when={virtualized() && range().bottomSpacerHeight > 0}>
                <tr aria-hidden="true" class="paged-result-spacer">
                  <td
                    colspan={props.columns.length +
                      (props.renderTrailingAction ? 1 : 0)}
                    style={{ height: `${range().bottomSpacerHeight}px` }}
                  />
                </tr>
              </Show>
            </tbody>
          </table>
        </div>
      </Show>
      <nav
        class="paged-result-pagination"
        aria-label={props.paginationLabel}
      >
        <button
          type="button"
          class="ui-button ui-button-secondary"
          disabled={!props.canPrevious || props.loading || !!props.error}
          onClick={() => props.onPrevious()}
        >
          {props.previousLabel}
        </button>
        <button
          type="button"
          class="ui-button ui-button-secondary"
          disabled={!props.canNext || props.loading || !!props.error}
          onClick={() => props.onNext()}
        >
          {props.nextLabel}
        </button>
      </nav>
    </section>
  );
}
