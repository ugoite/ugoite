import { For, Show } from "solid-js";
import type { JSX } from "solid-js";
import { RowListChevron } from "~/components/RowList";
import { createRowActionKeyHandler } from "~/lib/row-action-focus";
import type { SpaceChangeQueryRow } from "~/lib/ugoite-client";

export interface HistoryChangeColumn {
  key: string;
  label: string;
  cell: (row: SpaceChangeQueryRow) => JSX.Element;
}

export interface HistoryChangeTableProps {
  rows: readonly SpaceChangeQueryRow[];
  columns: readonly HistoryChangeColumn[];
  pageIdentity: string;
  tableLabel: string;
  selectedChangeId?: string;
  onSelectChange?: (row: SpaceChangeQueryRow) => void;
  openLabel: string;
  busy: boolean;
  onOpenChange?: (row: SpaceChangeQueryRow, opener: HTMLButtonElement) => void;
}

/**
 * Change-owned presentation for the current Space history page.
 * Renders Change display columns, selected-Change appearance, the trailing
 * Open Change control, sticky action column, and row-action keyboard
 * movement. Filters, sort, paging, loading, error, empty states, and
 * recovery stay with the history route: this table shows an already-fetched
 * page only. A Change is never an Entry row or a generic query row.
 */
export function HistoryChangeTable(props: HistoryChangeTableProps) {
  let tableElement: HTMLTableElement | undefined;
  const handleKeyDown = createRowActionKeyHandler(() => tableElement);

  return (
    <Show when={props.rows.length > 0}>
      <div
        class="result-table-viewport"
        data-page-identity={props.pageIdentity}
        aria-label={props.tableLabel}
      >
        <table
          ref={tableElement}
          class="result-table result-table--history history-change-table"
        >
          <thead>
            <tr>
              <For each={props.columns}>
                {(column) => (
                  <th scope="col" class="result-table-header">
                    {column.label}
                  </th>
                )}
              </For>
              <th scope="col" class="history-change-trailing-header">
                <span class="ui-sr-only">{props.openLabel}</span>
              </th>
            </tr>
          </thead>
          <tbody onKeyDown={handleKeyDown}>
            <For each={props.rows}>
              {(row, index) => (
                <tr
                  data-row-index={index()}
                  data-row-key={row.change_id}
                  data-change-id={row.change_id}
                  aria-selected={props.selectedChangeId === row.change_id ||
                    undefined}
                  onClick={() => props.onSelectChange?.(row)}
                  onFocusIn={() => props.onSelectChange?.(row)}
                >
                  <For each={props.columns}>
                    {(column) => (
                      <td
                        class="result-table-cell"
                        data-column-key={column.key}
                      >
                        {column.cell(row)}
                      </td>
                    )}
                  </For>
                  <td class="history-change-trailing-cell">
                    <button
                      type="button"
                      class="history-change-open"
                      aria-label={props.openLabel}
                      title={props.openLabel}
                      disabled={props.busy}
                      onClick={(event) => {
                        event.stopPropagation();
                        props.onOpenChange?.(row, event.currentTarget);
                      }}
                    >
                      <RowListChevron />
                    </button>
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>
    </Show>
  );
}
