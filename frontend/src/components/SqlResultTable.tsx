import { For, Show } from "solid-js";

export interface SqlResultTableProps {
  columns: readonly string[];
  rows: readonly unknown[];
  pageIdentity: string;
  tableLabel: string;
}

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
  return (
    <Show when={props.rows.length > 0}>
      <div
        class="result-table-viewport"
        data-page-identity={props.pageIdentity}
        aria-label={props.tableLabel}
      >
        <table class="result-table result-table--sql">
          <thead>
            <tr>
              <For each={props.columns}>
                {(column) => (
                  <th scope="col" class="result-table-header">
                    {column}
                  </th>
                )}
              </For>
            </tr>
          </thead>
          <tbody>
            <For each={props.rows}>
              {(row, rowIndex) => (
                <tr data-row-index={rowIndex()}>
                  <For each={props.columns}>
                    {(column, columnIndex) => {
                      const value = formatCell(
                        rowCell(row, column, columnIndex()),
                      );
                      return (
                        <td
                          class="result-table-cell"
                          data-column-key={column}
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
      </div>
    </Show>
  );
}
