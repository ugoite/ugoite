import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import type { DraftSource } from "~/lib/composition-draft";
import { CompositionDisplayPicker } from "./CompositionDisplayPicker";

const sqlSource: DraftSource = {
  kind: "saved_sql",
  draftId: "src-1",
  entryId: "sql-1",
  revisionId: "sql-rev-1",
  name: "Monthly",
  expectedResult: [
    { name: "total", type: "float" },
    { name: "payload", type: "json" },
  ],
  variables: {},
};

const entrySource: DraftSource = {
  kind: "entry_query",
  draftId: "src-2",
  formId: "11111111-1111-4111-8111-111111111111",
  name: "Tasks",
  fieldSchema: [
    { field_id: 3, field_type: "string" },
    { field_id: 4, field_type: "asset_reference" },
  ],
  query: { filters: [], sort: [], projection: { kind: "preview" } },
};

const jsonOnlySource: DraftSource = {
  kind: "saved_sql",
  draftId: "src-3",
  entryId: "sql-2",
  revisionId: "sql-rev-2",
  name: "Blobs",
  expectedResult: [{ name: "payload", type: "json" }],
  variables: {},
};

const stylesheet = () => readFileSync(join(__dirname, "..", "app.css"), "utf8");

describe("CompositionDisplayPicker", () => {
  beforeEach(() => {
    setLocale("en");
  });

  afterEach(() => cleanup());

  it("adds a table display from a draft source", () => {
    const onAdd = vi.fn();
    render(() => (
      <CompositionDisplayPicker
        kind="table"
        sources={[sqlSource, entrySource]}
        onAdd={onAdd}
        onClose={() => {}}
      />
    ));

    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Table" })).toBeNull();

    const dialog = screen.getByRole("dialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: /Monthly/ }),
    );

    const labelInput = within(dialog).getByLabelText("Label");
    fireEvent.input(labelInput, { target: { value: "Totals" } });
    const addButton = within(dialog).getByRole("button", {
      name: "Add display",
    });
    expect(addButton).toBeEnabled();
    fireEvent.click(addButton);

    expect(onAdd).toHaveBeenCalledTimes(1);
    expect(onAdd).toHaveBeenCalledWith({
      kind: "table",
      sourceDraftId: "src-1",
      label: "Totals",
    });
  });

  it("adds a metric display with scalar-only candidates", () => {
    const onAdd = vi.fn();
    render(() => (
      <CompositionDisplayPicker
        kind="metric"
        sources={[sqlSource, entrySource]}
        onAdd={onAdd}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog");
    expect(within(dialog).queryByRole("button", { name: "Metric" })).toBeNull();
    fireEvent.click(
      within(dialog).getByRole("button", { name: /Monthly/ }),
    );

    // json columns stay unselectable; the evaluator owns the rest.
    const valueSelect = within(dialog).getByLabelText(
      "Value",
    ) as HTMLSelectElement;
    const options = within(valueSelect).getAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual(["total"]);
    expect(valueSelect.value).toBe("total");

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add display" }),
    );
    expect(onAdd).toHaveBeenCalledWith({
      kind: "metric",
      sourceDraftId: "src-1",
      valueField: { column: "total" },
    });

    // Structurally non-scalar field types stay out of EntryQuery candidates.
    cleanup();
    const entryAdd = vi.fn();
    render(() => (
      <CompositionDisplayPicker
        kind="metric"
        sources={[entrySource]}
        fieldNames={(_formId, fieldId) => fieldId === 3 ? "Title" : undefined}
        onAdd={entryAdd}
        onClose={() => {}}
      />
    ));
    const entryDialog = screen.getByRole("dialog");
    fireEvent.click(
      within(entryDialog).getByRole("button", { name: /Tasks/ }),
    );
    const entrySelect = within(entryDialog).getByLabelText(
      "Value",
    ) as HTMLSelectElement;
    expect(
      within(entrySelect).getAllByRole("option").map((option) =>
        option.textContent
      ),
    ).toEqual(["Title"]);
    fireEvent.click(
      within(entryDialog).getByRole("button", { name: "Add display" }),
    );
    expect(entryAdd).toHaveBeenCalledWith({
      kind: "metric",
      sourceDraftId: "src-2",
      valueField: { fieldId: 3 },
    });
  });

  it("disables sources without scalar candidates", () => {
    const onAdd = vi.fn();
    render(() => (
      <CompositionDisplayPicker
        kind="metric"
        sources={[jsonOnlySource]}
        onAdd={onAdd}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog");
    const disabledRow = within(dialog).getByRole("button", {
      name: "Blobs",
    });
    expect(disabledRow).toBeDisabled();
    expect(disabledRow.textContent).toMatch(/No scalar values/);
    expect(
      within(dialog).queryByRole("button", { name: "Add display" }),
    ).toBeNull();
    expect(onAdd).not.toHaveBeenCalled();
  });

  it("focuses the first available source after disabled metric sources", async () => {
    render(() => (
      <CompositionDisplayPicker
        kind="metric"
        sources={[jsonOnlySource, sqlSource]}
        onAdd={() => {}}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByRole("button", { name: "Blobs" }))
      .toBeDisabled();
    const availableSource = within(dialog).getByRole("button", {
      name: "Monthly",
    });
    await waitFor(() => expect(document.activeElement).toBe(availableSource));
  });

  it("keeps the metric choice through field selection and uses form labels", () => {
    const onAdd = vi.fn();
    render(() => (
      <CompositionDisplayPicker
        kind="metric"
        sources={[entrySource]}
        fieldNames={(_formId, fieldId) => fieldId === 3 ? "Title" : undefined}
        onAdd={onAdd}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog");
    expect(within(dialog).queryByRole("button", { name: "Metric" })).toBeNull();
    fireEvent.click(within(dialog).getByRole("button", { name: /Tasks/ }));
    const valueSelect = within(dialog).getByLabelText("Value");
    expect(valueSelect).toHaveDisplayValue("Title");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add display" }),
    );
    expect(onAdd).toHaveBeenCalledWith({
      kind: "metric",
      sourceDraftId: "src-2",
      valueField: { fieldId: 3 },
    });
    expect(stylesheet()).toMatch(
      /\.ui-dialog\.composition-display-picker[\s\S]*?max-height:\s*calc\(100dvh - 32px\);[\s\S]*?overflow-y:\s*auto;/,
    );
    expect(stylesheet()).toMatch(
      /\.ui-dialog\.composition-display-picker[\s\S]*?width:\s*min\(560px,\s*calc\(100vw - 32px\)\)/,
    );
  });
});
