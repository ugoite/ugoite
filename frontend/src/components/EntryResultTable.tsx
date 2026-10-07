import { For, Show } from "solid-js";
import type { JSX } from "solid-js";
import type { EntryQueryResult } from "~/lib/entry-query";
import { createRowActionKeyHandler } from "~/lib/row-action-focus";
import { RowListChevron } from "./RowList";

export interface EntryResultColumn {
  key: string;
  label: string;
  cell: (row: EntryQueryResult) => JSX.Element;
}

export interface EntryResultTableProps {
  rows: readonly EntryQueryResult[];
  columns: readonly EntryResultColumn[];
  pageIdentity: string;
  tableLabel: string;
  selectedEntryId?: string;
  onSelectEntry?: (row: EntryQueryResult) => void;
  /** Trailing control variant. Omitted for read-only Entry presentation. */
  trailingAction?: "open" | "confirm";
  openLabel?: string;
  confirmLabel?: string;
  busy?: boolean;
  onOpenEntry?: (row: EntryQueryResult) => void;
}

/**
 * Entry-owned presentation for the current EntryQuery page.
 * Renders Entry display columns, selected-Entry appearance, the trailing
 * open/confirm control, sticky action column, and row-action keyboard
 * movement. Search, filter, sort, fetching, paging, loading, error, and
 * empty states stay with the owner: this table shows an already-fetched
 * page only. Row click selects; only the trailing control navigates.
 * Without selection and trailing action props the table is read-only
 * Entry presentation for embedding surfaces such as Compositions.
 */
export function EntryResultTable(props: EntryResultTableProps) {
  let tableElement: HTMLTableElement | undefined;
  const handleKeyDown = createRowActionKeyHandler(() => tableElement);
  const showTrailingAction = () =>
    props.trailingAction !== undefined && props.onOpenEntry !== undefined;

  return (
    <Show when={props.rows.length > 0}>
      <div
        class="result-table-viewport entry-browser-table-scroll"
        data-page-identity={props.pageIdentity}
        aria-label={props.tableLabel}
      >
        <table
          ref={tableElement}
          class="result-table result-table--entry entry-browser-table"
          classList={{ "result-table--static": props.onSelectEntry === undefined }}
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
              <Show when={showTrailingAction()}>
                <th scope="col" class="entry-browser-trailing-header">
                  <span class="ui-sr-only">
                    {props.trailingAction === "confirm"
                      ? props.confirmLabel
                      : props.openLabel}
                  </span>
                </th>
              </Show>
            </tr>
          </thead>
          <tbody onKeyDown={handleKeyDown}>
            <For each={props.rows}>
              {(row, index) => (
                <tr
                  data-row-index={index()}
                  data-row-key={row.id}
                  data-entry-id={row.id}
                  aria-selected={props.selectedEntryId === row.id || undefined}
                  onClick={() => props.onSelectEntry?.(row)}
                  onFocusIn={() => props.onSelectEntry?.(row)}
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
                  <Show when={showTrailingAction()}>
                    <td class="entry-browser-trailing-cell">
                      <button
                        type="button"
                        class="entry-browser-open"
                        aria-label={
                          props.trailingAction === "confirm"
                            ? props.confirmLabel
                            : props.openLabel
                        }
                        title={
                          props.trailingAction === "confirm"
                            ? props.confirmLabel
                            : props.openLabel
                        }
                        disabled={props.busy ?? false}
                        onClick={(event) => {
                          event.stopPropagation();
                          props.onOpenEntry?.(row);
                        }}
                      >
                        <RowListChevron />
                      </button>
                    </td>
                  </Show>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>
    </Show>
  );
}
