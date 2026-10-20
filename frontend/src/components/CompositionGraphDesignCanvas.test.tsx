import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CompositionGraphDesignCanvas } from "./CompositionGraphDesignCanvas";
import type { CompositionBindingProvider } from "~/lib/composition-variable-binding";
import { createCompositionGraphDraft } from "~/lib/composition-variable-binding";
import { setLocale } from "~/lib/i18n";

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
