import { createMemo, For, Show } from "solid-js";
import type { SqlResultColumnType } from "~/lib/types";

export interface SqlResultTableProps {
  columns: readonly string[];
  columnTypes?: readonly SqlResultColumnType[];
  /** Optional ordered display projection, keyed by exact source column name. */
  displayColumns?: readonly { column: string; label: string }[];
  rows: readonly unknown[];
  exactIntegerRows?: readonly (readonly (string | null)[])[];
  pageIdentity: string;
  tableLabel: string;
  /** Composition can keep its result headers and empty-state when there are no rows. */
  emptyMessage?: string;
  /** Fail closed when a configured display key cannot identify one source column. */
  invalidViewMessage?: string;
  viewportClassName?: string;
}

const numericColumnTypes = new Set<SqlResultColumnType>([
  "integer",
  "float",
  "decimal",
]);

const formatCell = (value: unknown): string => {
  if (value === null || value === undefined) return "—";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
};

const rowCell = (row: unknown, column: string, index: number): unknown => {
  if (Array.isArray(row)) return row[index];
  if (row && typeof row === "object") {
    return (row as Record<string, unknown>)[column];
  }
  return index === 0 ? row : undefined;
};

/**
 * SQL-owned presentation for the current SQL page.
 * Renders the exact column order with array/object row lookup and
 * scalar/structured display formatting. The table is stateless: query,
 * parameters, Saved SQL identity, count, Save as tool, paging, and
 * loading/error/empty states stay with the owning route or Composition
 * source. Narrow results fit the viewport; wide results scroll inside the
 * result viewport only.
 */
export function SqlResultTable(props: SqlResultTableProps) {
  const invalidView = createMemo(() =>
    props.displayColumns !== undefined &&
    (props.displayColumns.length === 0 ||
      props.displayColumns.some(({ column }) =>
        props.columns.filter((source) => source === column).length !== 1
      ))
  );
  const displayColumns = createMemo(() => {
    if (!props.displayColumns) {
      return props.columns.map((key, index) => ({ key, label: key, index }));
    }
    return props.displayColumns.flatMap(({ column, label }) => {
      const index = props.columns.indexOf(column);
      return index < 0 ? [] : [{ key: column, label, index }];
    });
  });
  const numericColumns = createMemo(() =>
    displayColumns().map(({ index }) => {
      const type = props.columnTypes?.[index];
      return type !== undefined && numericColumnTypes.has(type);
    })
  );

  return (
    <Show when={props.rows.length > 0 || props.emptyMessage !== undefined}>
      <div
        class={`result-table-viewport ${props.viewportClassName ?? ""}`}
        data-page-identity={props.pageIdentity}
        role="region"
        tabIndex={0}
        aria-label={props.tableLabel}
      >
        <Show
          when={!invalidView()}
          fallback={
            <p role="status">
              {props.invalidViewMessage ?? props.emptyMessage ??
                "Configured result columns are unavailable."}
            </p>
          }
        >
          <table
            class="result-table result-table--sql"
            aria-label={props.tableLabel}
          >
            <thead>
              <tr>
                <For each={displayColumns()}>
                  {(column, columnIndex) => (
                    <th
                      scope="col"
                      class="result-table-header"
                      classList={{
                        "result-table-header--numeric": numericColumns()[
                          columnIndex()
                        ],
                      }}
                    >
                      {column.label}
                    </th>
                  )}
                </For>
              </tr>
            </thead>
            <tbody>
              <For each={props.rows}>
                {(row, rowIndex) => (
                  <tr data-row-index={rowIndex()}>
                    <For each={displayColumns()}>
                      {(column, columnIndex) => {
                        const rowNumber = rowIndex();
                        const displayIndex = columnIndex();
                        const sourceIndex = column.index;
                        const exactIntegerValue =
                          props.columnTypes?.[sourceIndex] === "integer"
                            ? props.exactIntegerRows?.[rowNumber]?.[sourceIndex]
                            : undefined;
                        const value = formatCell(
                          exactIntegerValue !== undefined
                            ? exactIntegerValue
                            : rowCell(row, column.key, sourceIndex),
                        );
                        return (
                          <td
                            class="result-table-cell"
                            classList={{
                              "result-table-cell--numeric": numericColumns()[
                                displayIndex
                              ],
                            }}
                            data-column-key={column.key}
                          >
                            <span title={value}>{value}</span>
                          </td>
                        );
                      }}
                    </For>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </Show>
        <Show
          when={!invalidView() && props.rows.length === 0 && props.emptyMessage}
        >
          <p role="status">{props.emptyMessage}</p>
        </Show>
      </div>
    </Show>
  );
}
