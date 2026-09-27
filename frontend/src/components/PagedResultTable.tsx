import { type Accessor, For, type JSX, Show } from "solid-js";
import { LocalBusyIndicator } from "./LocalBusyIndicator";

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
  trailingActionClassName?: string;
  trailingHeaderClassName?: string;
  onRowSelect?: (row: Row, index: number) => void;
  selectedRowKey?: string;
  entryDataId?: (row: Row) => string;
  classNames?: { table?: string; scroll?: string };
  paginationLabel?: string;
}

export function PagedResultTable<Row>(props: PagedResultTableProps<Row>) {
  let tableElement: HTMLTableElement | undefined;
  const focusRow = (index: number) => {
    if (index < 0 || index >= props.rows.length) return;
    const focusTarget = () => {
      tableElement?.querySelector<HTMLElement>(
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
      classList={{
        "paged-result-row": true,
        "paged-result-row-selected": props.selectedRowKey ===
          props.rowKey(row, index()),
      }}
      data-row-index={index()}
      data-row-key={props.rowKey(row, index())}
      data-entry-id={props.entryDataId?.(row)}
      aria-selected={props.selectedRowKey === props.rowKey(row, index()) ||
        undefined}
      onClick={() => props.onRowSelect?.(row, index())}
      onFocusIn={() => props.onRowSelect?.(row, index())}
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
        <td
          class="paged-result-cell"
          classList={{
            [props.trailingActionClassName ?? ""]: !!props
              .trailingActionClassName,
          }}
        >
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
          class={props.classNames?.scroll ?? "paged-result-scroll"}
          data-page-identity={props.pageIdentity}
          aria-label={props.paginationLabel}
        >
          <table
            ref={tableElement}
            class={props.classNames?.table ?? "paged-result-table-element"}
          >
            <thead>
              <tr>
                <For each={props.columns}>
                  {(column) => <th scope="col">{column.label}</th>}
                </For>
                <Show when={props.renderTrailingAction}>
                  <th
                    scope="col"
                    class={props.trailingHeaderClassName}
                  >
                    <span class="ui-sr-only">
                      {props.trailingActionLabel}
                    </span>
                  </th>
                </Show>
              </tr>
            </thead>
            <tbody>
              <For each={props.rows}>{renderIndexedRow}</For>
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
          onClick={() =>
            props.onPrevious()}
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
