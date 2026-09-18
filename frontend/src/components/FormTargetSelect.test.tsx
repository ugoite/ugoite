import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, within } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { FormTargetSelect } from "./FormTargetSelect";

function setup(
  overrides: Partial<{
    value: string;
    options: string[];
    onChange: (value: string) => void;
  }> = {},
) {
  const onChange = overrides.onChange ?? vi.fn();
  render(() => (
    <FormTargetSelect
      label="Target Form"
      value={overrides.value ?? ""}
      options={overrides.options ?? ["Meeting", "Notes", "Project"]}
      placeholder="e.g. Project"
      onChange={onChange}
    />
  ));
  return { onChange };
}

describe("FormTargetSelect", () => {
  it("displays the stored value in a single combobox", () => {
    setup({ value: "Notes" });
    const box = screen.getByRole("combobox", { name: "Target Form" });
    expect(box).toHaveValue("Notes");
    // One box only: no separate search field plus selector.
    expect(screen.getAllByRole("combobox")).toHaveLength(1);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("filters candidates while typing", async () => {
    setup({ value: "" });
    const box = screen.getByRole("combobox", { name: "Target Form" });
    fireEvent.focus(box);
    fireEvent.input(box, { target: { value: "meet" } });
    const listbox = await screen.findByRole("listbox");
    const options = within(listbox).getAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual(["Meeting"]);
  });

  it("selects with mouse and stores the stable identifier", async () => {
    const onChange = vi.fn();
    setup({ value: "", onChange });
    const box = screen.getByRole("combobox", { name: "Target Form" });
    fireEvent.focus(box);
    fireEvent.input(box, { target: { value: "pro" } });
    const listbox = await screen.findByRole("listbox");
    fireEvent.click(within(listbox).getByRole("option", { name: "Project" }));
    expect(onChange).toHaveBeenCalledWith("Project");
  });

  it("selects with the keyboard", async () => {
    const onChange = vi.fn();
    setup({ value: "", onChange });
    const box = screen.getByRole("combobox", { name: "Target Form" });
    fireEvent.focus(box);
    fireEvent.keyDown(box, { key: "ArrowDown" });
    fireEvent.input(box, { target: { value: "note" } });
    fireEvent.keyDown(box, { key: "Enter" });
    expect(onChange).toHaveBeenCalledWith("Notes");
  });

  it("moves active option with arrow keys", async () => {
    const onChange = vi.fn();
    setup({ value: "", onChange });
    const box = screen.getByRole("combobox", { name: "Target Form" });
    fireEvent.focus(box);
    // Focus opens the popup at the first sorted option (Meeting);
    // one ArrowDown moves to Notes.
    fireEvent.keyDown(box, { key: "ArrowDown" });
    fireEvent.keyDown(box, { key: "Enter" });
    expect(onChange).toHaveBeenCalledWith("Notes");
  });

  it("reverts free text on blur instead of storing it", async () => {
    const onChange = vi.fn();
    setup({ value: "Notes", onChange });
    const box = screen.getByRole("combobox", { name: "Target Form" });
    fireEvent.focus(box);
    fireEvent.input(box, { target: { value: "Notes!!!" } });
    // focusout bubbles to the wrapper, which reverts uncommitted text.
    fireEvent.focusOut(box);
    expect(onChange).not.toHaveBeenCalled();
    expect(box).toHaveValue("Notes");
  });

  it("reverts on Escape", async () => {
    const onChange = vi.fn();
    setup({ value: "Notes", onChange });
    const box = screen.getByRole("combobox", { name: "Target Form" });
    fireEvent.focus(box);
    fireEvent.input(box, { target: { value: "xyz" } });
    fireEvent.keyDown(box, { key: "Escape" });
    expect(onChange).not.toHaveBeenCalled();
    expect(box).toHaveValue("Notes");
  });

  it("shows a no-result state", async () => {
    setup({ value: "" });
    const box = screen.getByRole("combobox", { name: "Target Form" });
    fireEvent.focus(box);
    fireEvent.input(box, { target: { value: "zzz" } });
    const listbox = await screen.findByRole("listbox");
    expect(
      within(listbox).getByRole("option", { name: "No matching forms." }),
    ).toBeInTheDocument();
  });

  it("shows an empty state when no Forms exist", async () => {
    setup({ value: "", options: [] });
    const box = screen.getByRole("combobox", { name: "Target Form" });
    fireEvent.focus(box);
    const listbox = await screen.findByRole("listbox");
    expect(
      within(listbox).getByRole("option", {
        name: "No forms available yet.",
      }),
    ).toBeInTheDocument();
  });

  it("surfaces a deleted target as an actionable unknown option", async () => {
    setup({ value: "Deleted", options: ["Meeting", "Notes"] });
    const box = screen.getByRole("combobox", { name: "Target Form" });
    expect(box).toHaveAttribute("aria-invalid", "true");
    fireEvent.focus(box);
    const listbox = await screen.findByRole("listbox");
    expect(
      within(listbox).getByRole("option", { name: "Unknown form: Deleted" }),
    ).toBeInTheDocument();
  });
});
