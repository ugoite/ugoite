import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CompositionVariableBindingPicker } from "./CompositionVariableBindingPicker";
import {
  type CompositionGraphDraft,
  createCompositionGraphDraft,
} from "~/lib/composition-variable-binding";
import { setLocale } from "~/lib/i18n";

const withTables = (): CompositionGraphDraft => ({
  ...createCompositionGraphDraft("Report"),
  variables: [
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
      id: "other_rows",
      kind: "query",
      returns: {
        kind: "table",
        columns: [{
          name: "id",
          value_type: { scalar_type: "integer", nullable: false },
        }],
      },
      query: { kind: "inline_sql", sql: "SELECT id", arguments: [] },
    },
  ],
});

describe("CompositionVariableBindingPicker", () => {
  beforeEach(() => setLocale("en"));
  afterEach(() => cleanup());

  it("places a selected table query as one component and layout item", () => {
    const draft = withTables();
    const onCommit = vi.fn();
    render(() => (
      <CompositionVariableBindingPicker
        draft={draft}
        onCommit={onCommit}
        onClose={vi.fn()}
      />
    ));
    const dialog = screen.getByRole("dialog", { name: "Table" });

    fireEvent.click(within(dialog).getByRole("button", { name: "rows" }));

    expect(onCommit).toHaveBeenCalledOnce();
    expect(onCommit.mock.calls[0][0].components).toEqual([{
      id: "component_1",
      kind: "table",
      variable: "rows",
    }]);
    expect(onCommit.mock.calls[0][0].layout.rows[0].items).toEqual([{
      kind: "component",
      component: "component_1",
    }]);
  });

  it("changes only the selected component reference and restores focus on close", async () => {
    const draft = withTables();
    draft.components.push({ id: "table_1", kind: "table", variable: "rows" });
    draft.layout.rows[0].items.push({
      kind: "component",
      component: "table_1",
    });
    const opener = document.createElement("button");
    document.body.append(opener);
    opener.focus();
    const onCommit = vi.fn();
    const onClose = vi.fn();
    const { unmount } = render(() => (
      <CompositionVariableBindingPicker
        draft={draft}
        componentId="table_1"
        onCommit={onCommit}
        onClose={onClose}
      />
    ));
    const dialog = screen.getByRole("dialog", { name: "Table" });
    const replacement = within(dialog).getByRole("button", {
      name: "other_rows",
    });
    fireEvent.click(replacement);

    expect(onCommit.mock.calls[0][0].components).toEqual([{
      id: "table_1",
      kind: "table",
      variable: "other_rows",
    }]);
    expect(onCommit.mock.calls[0][0].variables).toEqual(draft.variables);
    expect(onClose).toHaveBeenCalledOnce();
    unmount();
    await waitFor(() => expect(opener).toHaveFocus());
    opener.remove();
  });
});
