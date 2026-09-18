import "@testing-library/jest-dom/vitest";
import {
  fireEvent,
  render,
  screen,
} from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { describe, expect, it, vi } from "vitest";
import { ListEditor } from "./ListEditor";

function setup(values: string[] = ["alpha", "beta"]) {
  const onChange = vi.fn();
  render(() => (
    <ListEditor
      values={values}
      onChange={onChange}
      createItem={() => ""}
      addLabel="Add item"
      renderItem={(value, index, helpers) => (
        <div>
          <input
            aria-label={`Item ${index() + 1}`}
            value={value()}
            onInput={(event) =>
              onChange(
                values.map((entry, position) =>
                  position === index() ? event.currentTarget.value : entry
                ),
              )}
          />
          <button
            type="button"
            aria-label={`Remove item ${index() + 1}`}
            onClick={helpers.remove}
          >
            Remove
          </button>
        </div>
      )}
    />
  ));
  return { onChange };
}

function setupStateful(initial: string[] = ["a", "b", "c"]) {
  const [values, setValues] = createSignal<string[]>(initial);
  render(() => (
    <ListEditor
      values={values()}
      onChange={setValues}
      createItem={() => ""}
      addLabel="Add item"
      renderItem={(value, index, helpers) => (
        <div>
          <input
            aria-label={`Item ${index() + 1}`}
            value={value()}
            onInput={(event) =>
              setValues(
                values().map((entry, position) =>
                  position === index() ? event.currentTarget.value : entry
                ),
              )}
          />
          <button
            type="button"
            aria-label={`Remove item ${index() + 1}`}
            onClick={helpers.remove}
          >
            Remove
          </button>
        </div>
      )}
    />
  ));
  return { values };
}

describe("ListEditor", () => {
  it("renders one typed control per item in order", () => {
    setup();
    expect(screen.getByLabelText("Item 1")).toHaveValue("alpha");
    expect(screen.getByLabelText("Item 2")).toHaveValue("beta");
    // Keyboard order matches visual order.
    const inputs = document.querySelectorAll("input");
    expect(inputs).toHaveLength(2);
    expect(inputs[0]).toHaveValue("alpha");
    expect(inputs[1]).toHaveValue("beta");
  });

  it("appends a new item through the add action", () => {
    const { onChange } = setup(["alpha"]);
    fireEvent.click(screen.getByRole("button", { name: "Add item" }));
    expect(onChange).toHaveBeenCalledWith(["alpha", ""]);
  });

  it("removes the selected item", () => {
    const { onChange } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Remove item 1" }));
    expect(onChange).toHaveBeenCalledWith(["beta"]);
  });

  it("keeps the add action available with zero items", () => {
    setup([]);
    expect(screen.queryByLabelText(/Item/)).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Add item" }),
    ).toBeInTheDocument();
  });

  it("keeps focus while typing", () => {
    setupStateful(["alpha"]);
    const input = screen.getByLabelText("Item 1") as HTMLInputElement;
    input.focus();
    fireEvent.input(input, { target: { value: "alphab" } });
    expect(document.activeElement).toBe(input);
    expect(input).toHaveValue("alphab");
    fireEvent.input(input, { target: { value: "alphabc" } });
    expect(document.activeElement).toBe(input);
    expect(input).toHaveValue("alphabc");
  });

  it("removes the correct item after a prior removal", () => {
    setupStateful();
    // Remove the first item, then remove what is now second.
    fireEvent.click(screen.getByRole("button", { name: "Remove item 1" }));
    expect(screen.getByLabelText("Item 1")).toHaveValue("b");
    fireEvent.click(screen.getByRole("button", { name: "Remove item 2" }));
    expect(screen.getByLabelText("Item 1")).toHaveValue("b");
    expect(screen.queryByLabelText("Item 2")).not.toBeInTheDocument();
  });
});
