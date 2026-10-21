import "@testing-library/jest-dom/vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { describe, expect, it, vi } from "vitest";
import { CompositionVariableGraphWorkspace } from "./CompositionVariableGraphWorkspace";
import {
  type CompositionGraphDraft,
  createCompositionGraphDraft,
} from "~/lib/composition-variable-binding";
import { buildSqlSchema } from "~/lib/sql";
import { setLocale } from "~/lib/i18n";

const { parseInputMock } = vi.hoisted(() => ({ parseInputMock: vi.fn() }));

vi.mock("~/lib/ugoite-client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("~/lib/ugoite-client")>();
  return {
    ...actual,
    parseVariableGraphInput: (...args: unknown[]) => parseInputMock(...args),
  };
});

describe("CompositionVariableGraphWorkspace", () => {
  it("adds typed graph nodes through the variable rail", () => {
    setLocale("en");
    const [draft, setDraft] = createSignal<CompositionGraphDraft>(
      createCompositionGraphDraft("Report"),
    );
    const [selected, setSelected] = createSignal<string | null>(null);
    const onDraftChange = vi.fn(setDraft);
    render(() => (
      <CompositionVariableGraphWorkspace
        draft={draft()}
        selectedVariableId={selected()}
        onSelect={setSelected}
        onDraftChange={onDraftChange}
        onRemove={(id) =>
          setDraft({
            ...draft(),
            variables: draft().variables.filter((node) => node.id !== id),
          })}
        sqlSchema={buildSqlSchema([])}
      />
    ));

    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    fireEvent.click(screen.getByRole("button", { name: "Input" }));

    expect(draft().variables).toEqual([{
      id: "input_1",
      kind: "input",
      value_type: { scalar_type: "string", nullable: false },
      required: true,
    }]);
    expect(selected()).toBe("input_1");
    expect(screen.getByRole("combobox", { name: "Type" })).toBeVisible();

    fireEvent.change(screen.getByRole("combobox", { name: "Type" }), {
      target: { value: "date" },
    });
    expect(draft().variables[0]).toMatchObject({
      id: "input_1",
      kind: "input",
      value_type: { scalar_type: "date", nullable: false },
    });
    expect(onDraftChange).toHaveBeenCalled();
  });

  it("ignores a pending default parse after removing and re-adding the same ID", async () => {
    setLocale("en");
    let resolveParse!: (value: unknown) => void;
    parseInputMock.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveParse = resolve;
      }),
    );
    const [draft, setDraft] = createSignal<CompositionGraphDraft>({
      ...createCompositionGraphDraft("Report"),
      variables: [{
        id: "input_1",
        kind: "input",
        value_type: { scalar_type: "integer", nullable: false },
        required: true,
      }],
    });
    render(() => (
      <CompositionVariableGraphWorkspace
        draft={draft()}
        selectedVariableId="input_1"
        onSelect={() => {}}
        onDraftChange={setDraft}
        onRemove={(id) =>
          setDraft((current) => ({
            ...current,
            variables: current.variables.filter((node) => node.id !== id),
          }))}
        sqlSchema={buildSqlSchema([])}
      />
    ));

    fireEvent.input(screen.getByRole("textbox", { name: "Default" }), {
      target: { value: "42" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Remove input_1" }));
    expect(draft().variables).toEqual([]);

    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    fireEvent.click(screen.getByRole("button", { name: "Input" }));
    expect(draft().variables[0]).toMatchObject({
      id: "input_1",
      kind: "input",
      value_type: { scalar_type: "string", nullable: false },
      required: true,
    });

    resolveParse({ status: "valid", value: 42 });
    await Promise.resolve();

    expect(draft().variables[0]?.default).toBeUndefined();
    expect(screen.getByRole("textbox", { name: "Default" })).toHaveValue("");
  });

  it("ignores a pending default parse after the input type changes", async () => {
    setLocale("en");
    let resolveParse!: (value: unknown) => void;
    parseInputMock.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveParse = resolve;
      }),
    );
    const [draft, setDraft] = createSignal<CompositionGraphDraft>({
      ...createCompositionGraphDraft("Report"),
      variables: [{
        id: "minimum",
        kind: "input",
        value_type: { scalar_type: "integer", nullable: false },
        required: true,
      }],
    });
    render(() => (
      <CompositionVariableGraphWorkspace
        draft={draft()}
        selectedVariableId="minimum"
        onSelect={() => {}}
        onDraftChange={setDraft}
        onRemove={() => {}}
        sqlSchema={buildSqlSchema([])}
      />
    ));

    fireEvent.input(screen.getByRole("textbox", { name: "Default" }), {
      target: { value: "42" },
    });
    fireEvent.change(screen.getByRole("combobox", { name: "Type" }), {
      target: { value: "float" },
    });
    resolveParse({ status: "valid", value: 42 });
    await Promise.resolve();

    expect(draft().variables[0]).toMatchObject({
      id: "minimum",
      kind: "input",
      value_type: { scalar_type: "float", nullable: false },
    });
    expect(draft().variables[0]?.default).toBeUndefined();
    expect(screen.getByRole("textbox", { name: "Default" })).toHaveValue("");
  });

  it("commits a variable rename while the ID field is edited", () => {
    setLocale("en");
    const [draft, setDraft] = createSignal<CompositionGraphDraft>(
      createCompositionGraphDraft("Report"),
    );
    const [selected, setSelected] = createSignal<string | null>(null);
    render(() => (
      <CompositionVariableGraphWorkspace
        draft={draft()}
        selectedVariableId={selected()}
        onSelect={setSelected}
        onDraftChange={setDraft}
        onRemove={() => {}}
        sqlSchema={buildSqlSchema([])}
      />
    ));

    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    fireEvent.click(screen.getByRole("button", { name: "Input" }));
    fireEvent.input(screen.getByRole("textbox", { name: "Variable ID" }), {
      target: { value: "month_start" },
    });

    expect(draft().variables[0]?.id).toBe("month_start");
    expect(selected()).toBe("month_start");
    expect(screen.getByRole("button", { name: "month_start" }))
      .toBeVisible();
  });

  it("binds declared SQL arguments only to compatible graph variables", () => {
    setLocale("en");
    const draft = createCompositionGraphDraft("Report");
    draft.variables = [
      {
        id: "threshold",
        kind: "constant",
        value_type: { scalar_type: "integer", nullable: false },
        value: { kind: "integer", value: 3 },
      },
      {
        id: "caption",
        kind: "constant",
        value_type: { scalar_type: "string", nullable: false },
        value: { kind: "string", value: "Orders" },
      },
      {
        id: "query_1",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: {
          kind: "inline_sql",
          sql: "SELECT $limit",
          arguments: [{ name: "limit", variable: "" }],
        },
      },
    ];
    const onDraftChange = vi.fn();
    render(() => (
      <CompositionVariableGraphWorkspace
        draft={draft}
        selectedVariableId="query_1"
        onSelect={() => {}}
        onDraftChange={onDraftChange}
        onRemove={() => {}}
        sqlSchema={buildSqlSchema([])}
        parameterDeclarations={{
          query_1: [{ name: "limit", type: "integer" }],
        }}
      />
    ));

    const binding = screen.getByRole("combobox", { name: "limit" });
    expect(binding).toHaveValue("");
    expect(within(binding).getByRole("option", { name: "threshold" }))
      .toBeInTheDocument();
    expect(within(binding).queryByRole("option", { name: "caption" }))
      .toBeNull();

    fireEvent.change(binding, { target: { value: "threshold" } });

    expect(onDraftChange).toHaveBeenCalledOnce();
    expect(
      onDraftChange.mock.calls[0][0].variables.find((node) =>
        node.id === "query_1"
      ),
    ).toMatchObject({
      query: {
        arguments: [{ name: "limit", variable: "threshold" }],
      },
    });
  });

  it("traps and restores focus when the mobile variable drawer closes", async () => {
    setLocale("en");
    const draft = createCompositionGraphDraft("Report");
    render(() => (
      <div id="app">
        <CompositionVariableGraphWorkspace
          draft={draft}
          selectedVariableId={null}
          onSelect={() => {}}
          onDraftChange={() => {}}
          onRemove={() => {}}
          sqlSchema={buildSqlSchema([])}
        />
      </div>
    ));

    const opener = screen.getByRole("button", { name: "Show variables" });
    fireEvent.click(opener);
    const dialog = screen.getByRole("dialog", { name: "Variables" });
    expect(dialog).toBeVisible();
    expect(document.getElementById("app")).toHaveAttribute("inert");
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Variables" })).toBeNull();
    expect(document.getElementById("app")).not.toHaveAttribute("inert");
    await waitFor(() => expect(opener).toHaveFocus());
  });
});
