/**
 * Shared row-action keyboard vocabulary for native result tables.
 * Moves focus between the trailing (or primary) action buttons of adjacent
 * rows. This helper owns no rows, columns, paging, or selection: each
 * semantic table wires it to its own table element.
 */

const ROW_SELECTOR = "tr[data-row-index]";

const focusActionInRow = (
  root: ParentNode | undefined,
  index: number,
): boolean => {
  if (!root) return false;
  const target = root.querySelector<HTMLElement>(
    `${ROW_SELECTOR}[data-row-index="${index}"] button:not([disabled])`,
  );
  if (!target) return false;
  if (typeof requestAnimationFrame === "function") {
    requestAnimationFrame(() => target.focus());
  } else {
    target.focus();
  }
  return true;
};

export function focusRowAction(
  root: ParentNode | undefined,
  index: number,
): boolean {
  return focusActionInRow(root, index);
}

/**
 * Returns an ArrowUp/ArrowDown key handler that moves focus between row
 * actions inside the table returned by `table`. Events from outside a
 * `tr[data-row-index]` row, rows without a focusable button, and keys other
 * than ArrowUp/ArrowDown are ignored.
 */
export function createRowActionKeyHandler(
  table: () => Element | undefined,
): (event: KeyboardEvent) => void {
  return (event) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const target = event.target;
    if (!(target instanceof HTMLElement)) return;
    const row = target.closest<HTMLElement>(ROW_SELECTOR);
    if (!row || !row.querySelector("button:not([disabled])")) return;
    const index = Number(row.dataset.rowIndex);
    if (!Number.isInteger(index)) return;
    event.preventDefault();
    focusActionInRow(
      table() ?? undefined,
      index + (event.key === "ArrowDown" ? 1 : -1),
    );
  };
}
