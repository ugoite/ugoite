import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSignal } from "solid-js";
import { CompositionGraphDesignCanvas } from "./CompositionGraphDesignCanvas";
import {
  type CompositionGraphDraft,
  createCompositionGraphDraft,
} from "~/lib/composition-variable-binding";
import { setLocale } from "~/lib/i18n";

const { evaluateMock } = vi.hoisted(() => ({ evaluateMock: vi.fn() }));
vi.mock("~/lib/composition-api", () => ({
  compositionApi: {
    canonicalizeDocument: async (document: unknown) => ({
      canonical_yaml: JSON.stringify(document),
    }),
    evaluate: (...args: unknown[]) => evaluateMock(...args),
  },
}));

const baseProps = (draft: CompositionGraphDraft, onDraftChange: (draft: CompositionGraphDraft) => void) => ({
  spaceId: "space",
  draft,
  getPrincipalGeneration: async () => "principal",
  onDraftChange,
});

describe("CompositionGraphDesignCanvas", () => {
  beforeEach(() => {
    setLocale("en");
    evaluateMock.mockImplementation(async (
      _space: string,
      request: { evaluation_id: string },
    ) => ({
      evaluation_id: request.evaluation_id,
      ok: true,
      computed_query_ids: [],
      variable_values: {},
      queries: [],
      component_bindings: [],
      dependencies: {},
      authorization_fingerprint: "auth",
      publication_fingerprint: "publication",
      query_results: {},
    }));
  });
  afterEach(() => cleanup());

  it("edits Text directly, selects a local style, and inserts a scalar variable", async () => {
    const initial = createCompositionGraphDraft("Report");
    initial.variables.push({
      id: "total",
      kind: "constant",
      value_type: { scalar_type: "integer", nullable: false },
      value: { kind: "integer", value: "7" },
    });
    const [draft, setDraft] = createSignal(initial);
    render(() => <CompositionGraphDesignCanvas {...baseProps(draft(), setDraft)} />);

    fireEvent.click(screen.getByRole("button", { name: "Text" }));
    const editor = await screen.findByRole("textbox", { name: "Text" });
    fireEvent.input(editor, { target: { value: "Total: " } });
    fireEvent.change(screen.getByRole("combobox", { name: "Text style" }), {
      target: { value: "heading" },
    });
    fireEvent.change(screen.getByRole("combobox", { name: "Insert variable" }), {
      target: { value: "total" },
    });

    await waitFor(() => expect(draft().components).toEqual([{
      id: "text_1",
      kind: "text",
      content: "Total: {{total}}",
      style: "heading",
    }]));
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Text" })).toHaveFocus()
    );
  });

  it("selects an existing table variable and creates only its component binding", async () => {
    const initial = createCompositionGraphDraft("Report");
    initial.variables.push({
      id: "rows",
      kind: "query",
      returns: {
        kind: "table",
        columns: [{
          name: "name",
          value_type: { scalar_type: "string", nullable: false },
        }],
      },
      query: { kind: "inline_sql", sql: "SELECT name", arguments: [] },
    });
    const [draft, setDraft] = createSignal(initial);
    render(() => <CompositionGraphDesignCanvas {...baseProps(draft(), setDraft)} />);

    fireEvent.click(screen.getByRole("button", { name: "Table" }));
    const dialog = await screen.findByRole("dialog", { name: "Table" });
    fireEvent.click(within(dialog).getByRole("button", { name: "rows" }));

    await waitFor(() => expect(draft().components).toEqual([{
      id: "component_1",
      kind: "table",
      variable: "rows",
    }]));
    expect(draft().layout.rows[0].items).toEqual([{
      kind: "component",
      component: "component_1",
    }]);
  });

  it("places an existing Input variable from the palette", () => {
    const initial = createCompositionGraphDraft("Report");
    initial.variables.push({
      id: "minimum",
      kind: "input",
      value_type: { scalar_type: "integer", nullable: false },
      required: true,
    });
    const [draft, setDraft] = createSignal(initial);
    render(() => <CompositionGraphDesignCanvas {...baseProps(draft(), setDraft)} />);

    fireEvent.change(screen.getByRole("combobox", { name: "Input" }), {
      target: { value: "minimum" },
    });

    expect(draft().components).toEqual([{
      id: "input_1",
      kind: "input",
      variable: "minimum",
    }]);
  });
});
