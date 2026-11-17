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

  it("keeps the Text preview semantic and exposes a separate edit button", async () => {
    const initial = createCompositionGraphDraft("Report");
    initial.components.push({
      id: "headline",
      kind: "text",
      content: "Report title",
      style: "heading",
    });
    initial.layout.rows[0].items.push({
      kind: "component",
      component: "headline",
    });
    const [draft, setDraft] = createSignal(initial);
    const { container } = render(() => (
      <CompositionGraphDesignCanvas {...baseProps(draft(), setDraft)} />
    ));

    const preview = await screen.findByRole("heading", {
      name: "Report title",
    });
    expect(preview).toHaveClass("compositionGraphText");
    expect(container.querySelectorAll(".compositionGraphText")).toHaveLength(1);

    const editButton = screen.getByRole("button", { name: "Edit text" });
    expect(editButton).toHaveClass("compositionGraphTextSelect");
    expect(editButton).toHaveClass("compositionGraphSelect");
    fireEvent.click(editButton);

    expect(await screen.findByRole("textbox", { name: "Text" }))
      .toHaveValue("Report title");
  });

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
    editor.focus();
    fireEvent.input(editor, { target: { value: "Total: " } });
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Text" })).toHaveFocus()
    );
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

  it("preserves multiline Text through IME, Escape, and focus leaving the editor", async () => {
    const initial = createCompositionGraphDraft("Report");
    initial.components.push({
      id: "headline",
      kind: "text",
      content: "Initial",
      style: "body",
    });
    initial.layout.rows[0].items.push({
      kind: "component",
      component: "headline",
    });
    const [draft, setDraft] = createSignal(initial);
    render(() => <CompositionGraphDesignCanvas {...baseProps(draft(), setDraft)} />);

    fireEvent.click(await screen.findByRole("button", { name: "Edit text" }));
    const editor = await screen.findByRole("textbox", { name: "Text" });
    editor.focus();
    fireEvent.compositionStart(editor);
    fireEvent.keyDown(editor, { key: "Escape", isComposing: true });
    expect(editor).toBeInTheDocument();

    const multiline = "Line one\n二行目";
    fireEvent.input(editor, { target: { value: multiline } });
    fireEvent.compositionEnd(editor, { data: "二行目" });
    await waitFor(() =>
      expect(draft().components[0]).toMatchObject({ content: multiline })
    );

    const currentEditor = screen.getByRole("textbox", { name: "Text" });
    fireEvent.focusOut(currentEditor, { relatedTarget: document.body });
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Text" })).toHaveFocus()
    );
    currentEditor.focus();
    fireEvent.keyDown(currentEditor, { key: "Escape", isComposing: false });
    await waitFor(() =>
      expect(screen.queryByRole("textbox", { name: "Text" })).toBeNull()
    );
    const editButton = screen.getByRole("button", { name: "Edit text" });
    await waitFor(() => expect(editButton).toHaveFocus());

    fireEvent.click(editButton);
    const reopened = await screen.findByRole("textbox", { name: "Text" });
    expect(reopened).toHaveValue(multiline);
    reopened.focus();
    const addText = screen.getByRole("button", { name: "Text" });
    addText.focus();

    await waitFor(() =>
      expect(screen.queryByRole("textbox", { name: "Text" })).toBeNull()
    );
    expect(addText).toHaveFocus();
    expect(draft().components[0]).toMatchObject({ content: multiline });
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

  it("focuses the placed component itself when returning to Design", async () => {
    const initial = createCompositionGraphDraft("Report");
    initial.variables.push({
      id: "minimum",
      kind: "input",
      value_type: { scalar_type: "integer", nullable: false },
      required: true,
    });
    initial.components.push({ id: "input_1", kind: "input", variable: "minimum" });
    initial.layout.rows[0].items.push({
      kind: "component",
      component: "input_1",
    });
    const [draft, setDraft] = createSignal(initial);
    const [focusComponentId, setFocusComponentId] = createSignal<
      string | undefined
    >();
    const [focusGeneration, setFocusGeneration] = createSignal(0);
    const { container } = render(() => (
      <CompositionGraphDesignCanvas
        {...baseProps(draft(), setDraft)}
        focusComponentId={focusComponentId()}
        focusComponentGeneration={focusGeneration()}
      />
    ));

    const input = container.querySelector<HTMLElement>(
      '[data-component-id="input_1"][data-component-select="true"]',
    )!;
    expect(input).not.toHaveFocus();
    setFocusComponentId("input_1");
    setFocusGeneration(1);

    await waitFor(() => expect(input).toHaveFocus());
  });

  it("keeps a source-unavailable error global without showing an input-waiting query status", async () => {
    const initial = createCompositionGraphDraft("Report");
    initial.variables.push({
      id: "minimum",
      kind: "input",
      value_type: { scalar_type: "integer", nullable: false },
      required: true,
      default: { kind: "integer", value: "1" },
    });
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
      query: {
        kind: "inline_sql",
        sql: "SELECT name WHERE amount >= {{minimum}}",
        arguments: [{ name: "minimum", variable: "minimum" }],
      },
    });
    initial.components.push({ id: "table", kind: "table", variable: "rows" });
    initial.layout.rows[0].items.push({ kind: "component", component: "table" });
    const [draft, setDraft] = createSignal(initial);
    evaluateMock.mockImplementationOnce(async (
      _space: string,
      request: { evaluation_id: string },
    ) => ({
      evaluation_id: request.evaluation_id,
      ok: false,
      computed_query_ids: [],
      variable_values: {},
      queries: [],
      component_bindings: [],
      dependencies: {},
      authorization_fingerprint: "auth",
      publication_fingerprint: "publication",
      diagnostics: [{ code: "query_unavailable", variable_id: "rows" }],
      query_results: {},
    }));

    render(() => <CompositionGraphDesignCanvas {...baseProps(draft(), setDraft)} />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This source is unavailable or access may have changed. Choose a source again or check your access.",
    );
    expect(screen.queryByText("Waiting for input…")).toBeNull();
    expect(screen.queryByText("Loading results…")).toBeNull();
  });
});
