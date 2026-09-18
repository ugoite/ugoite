import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, within } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { ObjectListEditor } from "./ObjectListEditor";

function setup(
  values: Record<string, unknown>[] = [{ step: "one", count: 2, done: false }],
  onChange?: (values: Record<string, unknown>[]) => void,
) {
  const handleChange = onChange ?? vi.fn();
  render(() => (
    <ObjectListEditor
      fieldName="Checklist"
      values={values}
      onChange={handleChange}
      invalid={false}
      describedBy={undefined}
    />
  ));
  return { onChange: handleChange };
}

describe("ObjectListEditor", () => {
  it("renders one group per item with typed nested controls", () => {
    setup();
    const group = screen.getByRole("group", {
      name: "Checklist item 1",
    });
    expect(within(group).getByLabelText("step")).toHaveValue("one");
    expect(within(group).getByLabelText("count")).toHaveValue("2");
    expect(within(group).getByLabelText("done")).not.toBeChecked();
  });

  it("edits nested values without JSON", () => {
    const onChange = vi.fn();
    setup([{ step: "one" }], onChange);
    fireEvent.input(screen.getByLabelText("step"), {
      target: { value: "two" },
    });
    expect(onChange).toHaveBeenCalledWith([{ step: "two" }]);
  });

  it("toggles nested booleans", () => {
    const onChange = vi.fn();
    setup([{ done: false }], onChange);
    fireEvent.click(screen.getByLabelText("done"));
    expect(onChange).toHaveBeenCalledWith([{ done: true }]);
  });

  it("adds and removes properties", () => {
    const onChange = vi.fn();
    setup([{ step: "one" }], onChange);
    fireEvent.input(
      screen.getByLabelText("Property name"),
      { target: { value: "note" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Add property" }));
    expect(onChange).toHaveBeenCalledWith([{ step: "one", note: "" }]);

    fireEvent.click(
      screen.getByRole("button", { name: "Remove property step" }),
    );
    expect(onChange).toHaveBeenCalledWith([{}]);
  });

  it("adds and removes items without JSON", () => {
    const onChange = vi.fn();
    setup([], onChange);
    expect(screen.getByText("No items yet.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Add item" }));
    expect(onChange).toHaveBeenCalledWith([{}]);

    const { onChange: removeChange } = setup([{ step: "one" }]);
    fireEvent.click(
      screen.getByRole("button", { name: "Remove Checklist item 1" }),
    );
    expect(removeChange).toHaveBeenCalledWith([]);
  });
});
