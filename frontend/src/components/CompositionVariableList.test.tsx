import "@testing-library/jest-dom/vitest";
import { createSignal } from "solid-js";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { CompositionVariableList } from "./CompositionVariableList";
import type { DraftVariable } from "~/lib/composition-draft";
import * as variableProtocol from "~/lib/ugoite-client/protocol";

describe("composition variable list", () => {
  beforeEach(() => {
    setLocale("en");
  });
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  const month: DraftVariable = { id: "month", type: "date", required: true };

  it("shows one next-action line when empty", () => {
    render(() => (
      <CompositionVariableList
        variables={[]}
        headingId="variables"
        onAdd={() => {}}
        onUpdate={() => {}}
        onRemove={() => undefined}
      />
    ));
    expect(screen.getByText("Add a variable to begin.")).toBeInTheDocument();
  });

  it("adds a typed variable with an authored ID and no display label field", () => {
    const onAdd = vi.fn();
    render(() => (
      <CompositionVariableList
        variables={[]}
        headingId="variables"
        onAdd={onAdd}
        onUpdate={() => {}}
        onRemove={() => undefined}
      />
    ));

    fireEvent.input(screen.getByLabelText("Variable ID"), {
      target: { value: "revenue_total" },
    });
    fireEvent.change(screen.getByLabelText("Type"), {
      target: { value: "integer" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    expect(onAdd).toHaveBeenCalledWith({
      id: "revenue_total",
      type: "integer",
      required: true,
    });
    expect(screen.queryByLabelText("New variable label")).not
      .toBeInTheDocument();
  });

  it("blocks duplicate IDs and advances the default ID after adding", () => {
    const onAdd = vi.fn();
    render(() => (
      <CompositionVariableList
        variables={[{ id: "variable_1", type: "string", required: false }]}
        headingId="variables"
        onAdd={onAdd}
        onUpdate={() => {}}
        onRemove={() => undefined}
      />
    ));

    const idInput = screen.getByLabelText("Variable ID");
    expect(idInput).toHaveValue("variable_2");
    fireEvent.input(idInput, { target: { value: "variable_1" } });
    expect(screen.getByRole("button", { name: "Add variable" }))
      .toBeDisabled();
    fireEvent.input(idInput, { target: { value: "next_period" } });
    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    expect(onAdd).toHaveBeenCalledWith({
      id: "next_period",
      type: "string",
      required: true,
    });
  });

  it("shows stable IDs as variable names and edits typed defaults inline", () => {
    const onUpdate = vi.fn();
    render(() => (
      <CompositionVariableList
        variables={[month]}
        headingId="variables"
        onAdd={() => {}}
        onUpdate={onUpdate}
        onRemove={() => undefined}
      />
    ));
    expect(screen.getByText("month")).toBeInTheDocument();
    expect(screen.getByLabelText("Default for month")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Remove month" }))
      .toBeInTheDocument();
    fireEvent.input(screen.getByLabelText("Default for month"), {
      target: { value: "2026-10-01" },
    });
    expect(onUpdate).toHaveBeenCalledWith({ ...month, default: "2026-10-01" });
  });

  it("parses numeric defaults through the shared Domain parser", async () => {
    const integer: DraftVariable = {
      id: "limit",
      type: "integer",
      required: false,
      default: 0,
    };
    const onUpdate = vi.fn();
    render(() => (
      <CompositionVariableList
        variables={[integer]}
        headingId="variables"
        onAdd={() => {}}
        onUpdate={onUpdate}
        onRemove={() => undefined}
      />
    ));

    const input = screen.getByLabelText("Default for limit");
    expect(input).toHaveValue("0");
    expect(input).not.toHaveAttribute("readonly");
    fireEvent.input(input, { target: { value: "+00042" } });
    await waitFor(() => {
      expect(onUpdate).toHaveBeenLastCalledWith({ ...integer, default: 42 });
    });

    fireEvent.input(input, { target: { value: "1.0" } });
    await waitFor(() => {
      expect(onUpdate).toHaveBeenLastCalledWith({ ...integer, default: "1.0" });
    });
    expect(input).toHaveValue("1.0");
  });

  it("maps the boolean choice to its typed literal", () => {
    const boolean: DraftVariable = {
      id: "include_archived",
      type: "boolean",
      required: false,
      default: false,
    };
    const onUpdate = vi.fn();
    render(() => (
      <CompositionVariableList
        variables={[boolean]}
        headingId="variables"
        onAdd={() => {}}
        onUpdate={onUpdate}
        onRemove={() => undefined}
      />
    ));

    fireEvent.change(screen.getByLabelText("Default for include_archived"), {
      target: { value: "true" },
    });
    expect(onUpdate).toHaveBeenCalledWith({ ...boolean, default: true });
  });

  it("surfaces guarded removal without deleting", () => {
    const onRemove = vi.fn(() => "This variable is used by a data source.");
    render(() => (
      <CompositionVariableList
        variables={[month]}
        headingId="variables"
        onAdd={() => {}}
        onUpdate={() => {}}
        onRemove={onRemove}
      />
    ));
    fireEvent.click(screen.getByRole("button", { name: "Remove month" }));
    expect(onRemove).toHaveBeenCalledWith("month");
    expect(screen.getByText("This variable is used by a data source."))
      .toBeInTheDocument();
  });

  it("does not restore a removed variable when its default parse resolves", async () => {
    const integer: DraftVariable = {
      id: "limit",
      type: "integer",
      required: false,
    };
    let resolveParse!: (value: { status: "valid"; value: number }) => void;
    const pendingParse = new Promise<{ status: "valid"; value: number }>(
      (resolve) => {
        resolveParse = resolve;
      },
    );
    vi.spyOn(variableProtocol, "parseVariableGraphInput")
      .mockReturnValue(pendingParse as never);

    const [variables, setVariables] = createSignal<DraftVariable[]>([integer]);
    const onUpdate = vi.fn((updated: DraftVariable) => {
      setVariables((current) =>
        current.map((item) => item.id === updated.id ? updated : item)
      );
    });
    const onRemove = vi.fn((variableId: string) => {
      setVariables((current) =>
        current.filter((item) => item.id !== variableId)
      );
      return undefined;
    });

    render(() => (
      <CompositionVariableList
        variables={variables()}
        headingId="variables"
        onAdd={() => {}}
        onUpdate={onUpdate}
        onRemove={onRemove}
      />
    ));

    fireEvent.input(screen.getByLabelText("Default for limit"), {
      target: { value: "42" },
    });
    expect(onUpdate).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Remove limit" }));
    expect(onRemove).toHaveBeenCalledWith("limit");
    expect(variables()).toEqual([]);

    resolveParse({ status: "valid", value: 42 });
    await pendingParse;
    await waitFor(() => expect(variables()).toEqual([]));
    expect(onUpdate).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("limit")).not.toBeInTheDocument();
  });
});
