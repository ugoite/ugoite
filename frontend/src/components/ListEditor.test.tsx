import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
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
            aria-label={`Item ${index + 1}`}
            value={value}
            onInput={(event) =>
              onChange(
                values.map((entry, position) =>
                  position === index ? event.currentTarget.value : entry
                ),
              )}
          />
          <button
            type="button"
            aria-label={`Remove item ${index + 1}`}
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
});
