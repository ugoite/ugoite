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
        headingId="params"
        onAdd={() => {}}
        onUpdate={() => {}}
        onRemove={() => undefined}
      />
    ));
    expect(screen.getByText("Add a variable to begin.")).toBeInTheDocument();
  });

  it("creates a labeled variable without displaying its generated id", () => {
    const onAdd = vi.fn();
    render(() => (
      <CompositionVariableList
        variables={[]}
        headingId="params"
        onAdd={onAdd}
        onUpdate={() => {}}
        onRemove={() => undefined}
      />
    ));

    fireEvent.input(screen.getByLabelText("New variable label"), {
      target: { value: "Revenue" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    expect(onAdd).toHaveBeenCalledWith({
      id: "variable_1",
      type: "string",
      required: true,
      label: "Revenue",
    });
    expect(screen.queryByText("variable_1")).not.toBeInTheDocument();
  });

  it("selects an unused generated id without exposing it", () => {
    const onAdd = vi.fn();
    render(() => (
      <CompositionVariableList
        variables={[
          { id: "variable_1", type: "string", required: false },
          { id: "variable_3", type: "string", required: false },
        ]}
        headingId="params"
        onAdd={onAdd}
        onUpdate={() => {}}
        onRemove={() => undefined}
      />
    ));

    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    expect(onAdd).toHaveBeenCalledWith({
      id: "variable_2",
      type: "string",
      required: true,
    });
    expect(screen.queryByText("variable_1")).not.toBeInTheDocument();
    expect(screen.queryByText("variable_3")).not.toBeInTheDocument();
  });

  it("adds typed variables and edits defaults inline", () => {
    const onAdd = vi.fn();
    const onUpdate = vi.fn();
    render(() => (
      <CompositionVariableList
        variables={[month]}
        headingId="params"
        onAdd={onAdd}
        onUpdate={onUpdate}
        onRemove={() => undefined}
      />
    ));
    expect(screen.getByText("Variable 1")).toBeInTheDocument();
    expect(screen.queryByText("month")).not.toBeInTheDocument();

    fireEvent.input(screen.getByLabelText("New variable label"), {
      target: { value: "Region" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    expect(onAdd).toHaveBeenCalledWith({
      id: "variable_1",
      type: "string",
      required: true,
      label: "Region",
    });

    const defaultInput = screen.getByLabelText("Default for Variable 1");
    fireEvent.input(defaultInput, { target: { value: "2026-10-01" } });
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
        headingId="params"
        onAdd={() => {}}
        onUpdate={onUpdate}
        onRemove={() => undefined}
      />
    ));

    const input = screen.getByLabelText("Default for Variable 1");
    expect(input).toHaveValue("0");
    expect(input).not.toHaveAttribute("readonly");
    fireEvent.input(input, { target: { value: "+00042" } });
    await waitFor(() => {
      expect(onUpdate).toHaveBeenLastCalledWith({ ...integer, default: 42 });
    });

    fireEvent.input(input, { target: { value: "1.0" } });
    await waitFor(() => {
      expect(onUpdate).toHaveBeenLastCalledWith({
        ...integer,
        default: "1.0",
      });
    });
    expect(input).toHaveValue("1.0");
  });

  it("maps the boolean choice to its typed literal without parsing arbitrary input", () => {
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
        headingId="params"
        onAdd={() => {}}
        onUpdate={onUpdate}
        onRemove={() => undefined}
      />
    ));

    fireEvent.change(screen.getByLabelText("Default for Variable 1"), {
      target: { value: "true" },
    });
    expect(onUpdate).toHaveBeenCalledWith({ ...boolean, default: true });
  });

  it("uses localized ordinal labels instead of unlabeled variable keys", () => {
    const region: DraftVariable = {
      id: "region_key",
      type: "string",
      required: false,
    };
    render(() => (
      <CompositionVariableList
        variables={[month, region]}
        headingId="params"
        onAdd={() => {}}
        onUpdate={() => {}}
        onRemove={() => undefined}
      />
    ));

    expect(screen.getByText("Variable 1")).toBeInTheDocument();
    expect(screen.getByText("Variable 2")).toBeInTheDocument();
    expect(screen.queryByText("month")).not.toBeInTheDocument();
    expect(screen.queryByText("region_key")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Default for Variable 2"))
      .toBeInTheDocument();

    setLocale("ja");
    expect(screen.getByText("変数1")).toBeInTheDocument();
    expect(screen.getByText("変数2")).toBeInTheDocument();
    expect(screen.getByLabelText("変数2の既定値"))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "変数1を削除" }))
      .toBeInTheDocument();
  });

  it("uses the declared label while keeping the variable name for updates and removal", () => {
    const labeledMonth: DraftVariable = {
      ...month,
      id: "month_key",
      label: "Month",
    };
    const onUpdate = vi.fn();
    const onRemove = vi.fn(() => undefined);
    render(() => (
      <CompositionVariableList
        variables={[labeledMonth]}
        headingId="params"
        onAdd={() => {}}
        onUpdate={onUpdate}
        onRemove={onRemove}
      />
    ));

    expect(screen.getByText("Month")).toBeInTheDocument();
    expect(screen.queryByText("month_key")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Label for Month")).toHaveValue("Month");
    expect(screen.queryByLabelText("Label for month_key")).not
      .toBeInTheDocument();
    expect(screen.queryByLabelText("Default for month_key")).not
      .toBeInTheDocument();
    fireEvent.input(screen.getByLabelText("Default for Month"), {
      target: { value: "2026-10-01" },
    });
    expect(onUpdate).toHaveBeenCalledWith({
      ...labeledMonth,
      default: "2026-10-01",
    });

    fireEvent.click(screen.getByRole("button", { name: "Remove Month" }));
    expect(onRemove).toHaveBeenCalledWith("month_key");
  });

  it("surfaces guarded removal without deleting", () => {
    const onRemove = vi.fn(() => "This variable is used by a data source.");
    render(() => (
      <CompositionVariableList
        variables={[month]}
        headingId="params"
        onAdd={() => {}}
        onUpdate={() => {}}
        onRemove={onRemove}
      />
    ));
    fireEvent.click(
      screen.getByRole("button", { name: "Remove Variable 1" }),
    );
    expect(onRemove).toHaveBeenCalledWith("month");
    expect(
      screen.getByText("This variable is used by a data source."),
    ).toBeInTheDocument();
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
    vi.spyOn(variableProtocol, "parseCompositionVariableInput")
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
        headingId="params"
        onAdd={() => {}}
        onUpdate={onUpdate}
        onRemove={onRemove}
      />
    ));

    fireEvent.input(screen.getByLabelText("Default for Variable 1"), {
      target: { value: "42" },
    });
    expect(onUpdate).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Remove Variable 1" }));
    expect(onRemove).toHaveBeenCalledWith("limit");
    expect(variables()).toEqual([]);

    resolveParse({ status: "valid", value: 42 });
    await pendingParse;
    await waitFor(() => expect(variables()).toEqual([]));

    expect(onUpdate).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("Variable 1")).not.toBeInTheDocument();
  });
});
