import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { CompositionParameterList } from "./CompositionParameterList";
import type { DraftParameter } from "~/lib/composition-draft";

describe("composition parameter list", () => {
  beforeEach(() => {
    setLocale("en");
  });
  afterEach(() => cleanup());

  const month: DraftParameter = { id: "month", type: "date", required: true };

  it("shows one next-action line when empty", () => {
    render(() => (
      <CompositionParameterList
        parameters={[]}
        headingId="params"
        referencedIds={new Set()}
        onAdd={() => {}}
        onUpdate={() => {}}
        onRemove={() => undefined}
      />
    ));
    expect(screen.getByText("Add a parameter to begin.")).toBeInTheDocument();
  });

  it("names the disabled reason on the add control without prose", () => {
    render(() => (
      <CompositionParameterList
        parameters={[]}
        headingId="params"
        referencedIds={new Set()}
        onAdd={() => {}}
        onUpdate={() => {}}
        onRemove={() => undefined}
      />
    ));

    // An empty id needs a parameter name; the reason reuses the existing
    // parameter-name vocabulary in the accessible name and title.
    const empty = screen.getByRole("button", {
      name: "Add parameter: Parameter name",
    });
    expect(empty).toBeDisabled();
    expect(empty).toHaveAttribute("title", "Add parameter: Parameter name");
  });

  it("names the colliding id when the parameter already exists", () => {
    render(() => (
      <CompositionParameterList
        parameters={[month]}
        headingId="params"
        referencedIds={new Set(["month"])}
        onAdd={() => {}}
        onUpdate={() => {}}
        onRemove={() => undefined}
      />
    ));

    const idInput = screen.getByLabelText("Parameter name");
    fireEvent.input(idInput, { target: { value: "month" } });
    const taken = screen.getByRole("button", {
      name: "Add parameter: month",
    });
    expect(taken).toBeDisabled();
    expect(taken).toHaveAttribute("title", "Add parameter: month");
  });

  it("adds typed parameters and edits defaults inline", () => {
    const onAdd = vi.fn();
    const onUpdate = vi.fn();
    render(() => (
      <CompositionParameterList
        parameters={[month]}
        headingId="params"
        referencedIds={new Set(["month"])}
        onAdd={onAdd}
        onUpdate={onUpdate}
        onRemove={() => undefined}
      />
    ));
    expect(screen.getByText("month")).toBeInTheDocument();

    const idInput = screen.getByLabelText("Parameter name");
    fireEvent.input(idInput, { target: { value: "region" } });
    fireEvent.click(screen.getByRole("button", { name: "Add parameter" }));
    expect(onAdd).toHaveBeenCalledWith({
      id: "region",
      type: "string",
      required: true,
    });

    const defaultInput = screen.getByLabelText("Default for month");
    fireEvent.change(defaultInput, { target: { value: "2026-10-01" } });
    expect(onUpdate).toHaveBeenCalledWith({ ...month, default: "2026-10-01" });
  });

  it("surfaces guarded removal without deleting", () => {
    const onRemove = vi.fn(() => "This parameter is used by a data source.");
    render(() => (
      <CompositionParameterList
        parameters={[month]}
        headingId="params"
        referencedIds={new Set(["month"])}
        onAdd={() => {}}
        onUpdate={() => {}}
        onRemove={onRemove}
      />
    ));
    fireEvent.click(screen.getByRole("button", { name: "Remove month" }));
    expect(onRemove).toHaveBeenCalledWith("month");
    expect(
      screen.getByText("This parameter is used by a data source."),
    ).toBeInTheDocument();
  });
});
