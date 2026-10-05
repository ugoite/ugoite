import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import {
  createRowActionKeyHandler,
  focusRowAction,
} from "./row-action-focus";

const renderRows = (options?: { disabledFirst?: boolean }) => {
  const onKeyDown = vi.fn();
  let table: HTMLTableElement | undefined;
  render(() => (
    <table
      ref={table}
      onKeyDown={(event) => {
        onKeyDown(event.key);
        createRowActionKeyHandler(() => table)(event);
      }}
    >
      <tbody>
        <tr data-row-index="0">
          <td>
            <button type="button" disabled={options?.disabledFirst}>
              Open row-0
            </button>
          </td>
        </tr>
        <tr data-row-index="1">
          <td>
            <button type="button">Open row-1</button>
          </td>
        </tr>
      </tbody>
    </table>
  ));
  return { onKeyDown, table: () => table };
};

describe("row-action-focus", () => {
  it("moves focus to the action in the next and previous rows", async () => {
    renderRows();
    const first = screen.getByRole("button", { name: "Open row-0" });
    fireEvent.keyDown(first, { key: "ArrowDown" });
    const second = screen.getByRole("button", { name: "Open row-1" });
    await waitFor(() => expect(second).toHaveFocus());
    fireEvent.keyDown(second, { key: "ArrowUp" });
    await waitFor(() => expect(first).toHaveFocus());
  });

  it("ignores events from outside a data row", () => {
    const { onKeyDown, table } = renderRows();
    const focused = screen.getByRole("button", { name: "Open row-0" });
    focused.focus();
    fireEvent.keyDown(table()!, { key: "ArrowDown" });
    expect(onKeyDown).toHaveBeenCalledWith("ArrowDown");
    expect(focused).toHaveFocus();
    expect(focusRowAction(document.body, 0)).toBe(true);
  });

  it("ignores keys outside ArrowUp and ArrowDown", () => {
    const { onKeyDown } = renderRows();
    const first = screen.getByRole("button", { name: "Open row-0" });
    first.focus();
    fireEvent.keyDown(first, {
      key: "Enter",
    });
    expect(onKeyDown).toHaveBeenCalledWith("Enter");
    expect(first).toHaveFocus();
  });

  it("keeps focus when no adjacent row action exists", async () => {
    renderRows();
    const first = screen.getByRole("button", { name: "Open row-0" });
    first.focus();
    fireEvent.keyDown(first, { key: "ArrowUp" });
    await waitFor(() => expect(first).toHaveFocus());
    expect(focusRowAction(undefined, 0)).toBe(false);
    expect(focusRowAction(document.createElement("div"), 7)).toBe(false);
  });

  it("skips a disabled adjacent action", async () => {
    renderRows({ disabledFirst: true });
    const second = screen.getByRole("button", { name: "Open row-1" });
    second.focus();
    fireEvent.keyDown(second, { key: "ArrowUp" });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(second).toHaveFocus();
  });
});
