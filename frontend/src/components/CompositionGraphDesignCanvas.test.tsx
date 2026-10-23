import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSignal } from "solid-js";
import { CompositionGraphDesignCanvas } from "./CompositionGraphDesignCanvas";
import type { CompositionBindingProvider } from "~/lib/composition-variable-binding";
import { createCompositionGraphDraft } from "~/lib/composition-variable-binding";
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

describe("CompositionGraphDesignCanvas", () => {
  beforeEach(() => setLocale("en"));
  afterEach(() => cleanup());

  const provider: CompositionBindingProvider = {
    resolveForm: vi.fn(),
    prepareSavedSql: vi.fn(),
  };
  const renderCanvas = (
    onDraftChange = vi.fn(),
    draft = createCompositionGraphDraft("Report"),
  ) =>
    render(() => (
      <CompositionGraphDesignCanvas
        draft={draft}
        forms={[]}
        savedSql={[]}
        provider={provider}
        session={{
          session_id: "session",
          space_id: "space",
          document_revision: null,
          principal_generation: "principal",
        }}
        getPrincipalGeneration={async () => "principal"}
        isSessionCurrent={() => true}
        onDraftChange={onDraftChange}
      />
    ));

  it("renders real values, reorders the shared layout, and preserves variables when removing displays", async () => {
    const initial = createCompositionGraphDraft("Live");
    initial.variables = [
      {
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
      },
      {
        id: "total",
        kind: "constant",
        value_type: { scalar_type: "integer", nullable: false },
        value: { kind: "integer", value: 7 },
      },
      {
        id: "minimum",
        kind: "input",
        value_type: { scalar_type: "integer", nullable: false },
        required: true,
        default: { kind: "integer", value: 1 },
      },
    ];
    initial.components = [
      { id: "table", kind: "table", variable: "rows" },
      { id: "metric", kind: "metric", variable: "total" },
      { id: "input", kind: "input", variable: "minimum" },
      {
        id: "text",
        kind: "text",
        content: { kind: "literal", text: "Report title" },
        style: "title",
      },
    ];
    initial.layout.rows[0].items = initial.components.map((item) => ({
      kind: "component",
      component: item.id,
    }));
    evaluateMock.mockImplementation(async (
      _space: string,
      request: { evaluation_id: string },
    ) => ({
      evaluation_id: request.evaluation_id,
      ok: true,
      computed_query_ids: ["rows"],
      variable_values: {},
      queries: [],
      component_bindings: [],
      dependencies: {},
      authorization_fingerprint: "auth",
      publication_fingerprint: "pub",
      query_results: {
        rows: {
          value: {
            kind: "table",
            columns: [{
              name: "name",
              value_type: { scalar_type: "string", nullable: false },
            }],
            rows: [[{ kind: "string", value: "Alpha" }]],
          },
          has_more: false,
        },
      },
    }));
    const [draft, setDraft] = createSignal(initial);
    const { container } = render(() => (
      <CompositionGraphDesignCanvas
        draft={draft()}
        forms={[]}
        savedSql={[]}
        provider={provider}
        session={{
          session_id: "live",
          space_id: "space",
          document_revision: null,
          principal_generation: "p",
        }}
        getPrincipalGeneration={async () => "p"}
        isSessionCurrent={() => true}
        onDraftChange={setDraft}
      />
    ));
    expect(await screen.findByRole("table", { name: "rows results" }))
      .toHaveTextContent("Alpha");
    expect(screen.getByLabelText("total")).toHaveTextContent("7");
    expect(screen.getByLabelText("minimum")).toHaveValue("1");
    expect(screen.getByRole("heading", { name: "Report title" })).toHaveClass(
      "compositionGraphText--title",
    );
    fireEvent.click(screen.getByRole("button", { name: "metric, total" }));
    const move = screen.getByRole("button", { name: "Move earlier" });
    move.focus();
    fireEvent.click(move);
    expect(draft().layout.rows[0].items.map((item) => item.component)).toEqual([
      "metric",
      "table",
      "input",
      "text",
    ]);
    expect(
      [...container.querySelectorAll("[data-component-id]")].map((item) =>
        item.getAttribute("data-component-id")
      ),
    ).toEqual(["metric", "table", "input", "text"]);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "metric, total" }))
        .toHaveFocus()
    );
    fireEvent.click(screen.getByRole("button", { name: "Move to new row" }));
    expect(draft().layout.rows.at(-1)?.items).toEqual([{
      kind: "component",
      component: "metric",
    }]);
    fireEvent.click(
      screen.getByRole("button", { name: "Remove total" }),
    );
    expect(draft().components.some((item) => item.id === "metric")).toBe(false);
    expect(draft().variables).toEqual(initial.variables);
    expect(evaluateMock).toHaveBeenCalledOnce();
    expect(JSON.stringify(draft())).not.toContain("Alpha");
  });

  it("inserts literal text without selecting it for Inspector editing", () => {
    const onDraftChange = vi.fn();
    renderCanvas(onDraftChange);
    fireEvent.click(screen.getByRole("button", { name: "Add to design" }));
    fireEvent.click(screen.getByRole("button", { name: "Text" }));
    expect(onDraftChange).toHaveBeenCalledOnce();
    expect(onDraftChange.mock.calls[0][0].components).toEqual([{
      id: "text_1",
      kind: "text",
      content: { kind: "literal", text: "" },
      style: "body",
    }]);
    expect(screen.queryByRole("textbox", { name: "Text" })).not
      .toBeInTheDocument();
  });

  it("opens the single typed variable picker for Table and exposes only variable binding in Inspector", () => {
    const draft = createCompositionGraphDraft("Report");
    draft.variables.push({
      id: "rows",
      kind: "query",
      returns: {
        kind: "table",
        columns: [{
          name: "name",
          value_type: { scalar_type: "string", nullable: false },
        }],
      },
      query: { kind: "inline_sql", sql: "SELECT 1", arguments: [] },
    });
    draft.variables.push({
      id: "total",
      kind: "query",
      returns: {
        kind: "scalar",
        value_type: { scalar_type: "integer", nullable: false },
      },
      query: { kind: "inline_sql", sql: "SELECT COUNT(*)", arguments: [] },
    });
    draft.components.push({ id: "table_1", kind: "table", variable: "rows" });
    draft.layout.rows[0].items.push({
      kind: "component",
      component: "table_1",
    });
    renderCanvas(vi.fn(), draft);
    fireEvent.click(screen.getByRole("button", { name: "table, rows" }));
    expect(screen.getByRole("button", { name: "Change variable, rows" }))
      .toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Change variable, rows" }),
    );
    expect(screen.getByRole("dialog", { name: "Table" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Variables" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("button", { name: "rows" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "total" })).not
      .toBeInTheDocument();
    expect(screen.queryByText("Source")).not.toBeInTheDocument();
  });

  it("closes the binding picker on Escape and restores focus", async () => {
    const draft = createCompositionGraphDraft("Report");
    draft.variables.push({
      id: "rows",
      kind: "query",
      returns: {
        kind: "table",
        columns: [{
          name: "name",
          value_type: { scalar_type: "string", nullable: false },
        }],
      },
      query: { kind: "inline_sql", sql: "SELECT 1", arguments: [] },
    });
    draft.components.push({ id: "table_1", kind: "table", variable: "rows" });
    draft.layout.rows[0].items.push({
      kind: "component",
      component: "table_1",
    });
    renderCanvas(vi.fn(), draft);
    fireEvent.click(screen.getByRole("button", { name: "table, rows" }));
    const opener = screen.getByRole("button", {
      name: "Change variable, rows",
    });
    opener.focus();
    fireEvent.click(opener);
    const dialog = screen.getByRole("dialog", { name: "Table" });

    fireEvent.keyDown(dialog, { key: "Escape" });

    expect(screen.queryByRole("dialog", { name: "Table" })).toBeNull();
    await waitFor(() => expect(opener).toHaveFocus());
  });
});
