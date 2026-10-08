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
    { field_id: 100, field_type: "string" },
    { field_id: 101, field_type: "asset_reference" },
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
        sources={[sqlSource, entrySource]}
        onAdd={onAdd}
        onClose={() => {}}
      />
    ));

    expect(screen.getByRole("dialog")).toBeInTheDocument();

    const dialog = screen.getByRole("dialog");
    const kindGroup = within(dialog).getByRole("tablist", {
      name: "Display type",
    });
    expect(within(kindGroup).getByRole("tab", { name: "Table" }))
      .toHaveAttribute("aria-selected", "true");
    expect(within(dialog).queryByLabelText("Value")).toBeNull();
    fireEvent.click(
      within(dialog).getByRole("button", { name: /Monthly/ }),
    );
    expect(dialog).not.toHaveTextContent(sqlSource.draftId);
    expect(dialog).not.toHaveTextContent(sqlSource.entryId);
    expect(dialog).not.toHaveTextContent(sqlSource.revisionId);

    const labelInput = within(dialog).getByLabelText("Label");
    fireEvent.input(labelInput, { target: { value: "Totals" } });
    const addButton = within(dialog).getByRole("button", {
      name: "Add",
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
        sources={[sqlSource, entrySource]}
        onAdd={onAdd}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog");
    fireEvent.click(within(dialog).getByRole("tab", { name: "Metric" }));
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
      within(dialog).getByRole("button", { name: "Add" }),
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
        sources={[entrySource]}
        fieldNames={(_formId, fieldId) => fieldId === 100 ? "Title" : undefined}
        onAdd={entryAdd}
        onClose={() => {}}
      />
    ));
    const entryDialog = screen.getByRole("dialog");
    fireEvent.click(
      within(entryDialog).getByRole("tab", { name: "Metric" }),
    );
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
    expect(entryDialog).not.toHaveTextContent(entrySource.formId);
    expect(entryDialog).not.toHaveTextContent("100");
    fireEvent.click(
      within(entryDialog).getByRole("button", { name: "Add" }),
    );
    expect(entryAdd).toHaveBeenCalledWith({
      kind: "metric",
      sourceDraftId: "src-2",
      valueField: { fieldId: 100 },
    });
  });

  it("disables sources without scalar candidates", () => {
    const onAdd = vi.fn();
    render(() => (
      <CompositionDisplayPicker
        sources={[jsonOnlySource]}
        onAdd={onAdd}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog");
    fireEvent.click(within(dialog).getByRole("tab", { name: "Metric" }));
    const disabledRow = within(dialog).getByRole("button", {
      name: "Blobs",
    });
    expect(disabledRow).toBeDisabled();
    expect(disabledRow.textContent).toMatch(/No scalar values/);
    expect(
      within(dialog).queryByRole("button", { name: "Add" }),
    ).toBeNull();
    expect(onAdd).not.toHaveBeenCalled();
  });

  it("keeps the display kind selected while metric sources are filtered", async () => {
    render(() => (
      <CompositionDisplayPicker
        sources={[jsonOnlySource, sqlSource]}
        onAdd={() => {}}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog");
    const metricButton = within(dialog).getByRole("tab", {
      name: "Metric",
    });
    fireEvent.click(metricButton);
    expect(metricButton).toHaveAttribute("aria-selected", "true");
    expect(within(dialog).getByRole("button", { name: "Blobs" }))
      .toBeDisabled();
    const availableSource = within(dialog).getByRole("button", {
      name: "Monthly",
    });
    expect(availableSource).toBeEnabled();
    await waitFor(() =>
      expect(metricButton).toHaveAttribute("aria-selected", "true")
    );
  });

  it("selects Metric in the picker and uses Form labels", () => {
    const onAdd = vi.fn();
    render(() => (
      <CompositionDisplayPicker
        sources={[entrySource]}
        fieldNames={(_formId, fieldId) => fieldId === 100 ? "Title" : undefined}
        onAdd={onAdd}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog");
    fireEvent.click(within(dialog).getByRole("tab", { name: "Metric" }));
    fireEvent.click(within(dialog).getByRole("button", { name: /Tasks/ }));
    const valueSelect = within(dialog).getByLabelText("Value");
    expect(valueSelect).toHaveDisplayValue("Title");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add" }),
    );
    expect(onAdd).toHaveBeenCalledWith({
      kind: "metric",
      sourceDraftId: "src-2",
      valueField: { fieldId: 100 },
    });
    expect(stylesheet()).toMatch(
      /\.ui-dialog\.composition-display-picker[\s\S]*?max-height:\s*calc\(100dvh - 32px\);[\s\S]*?overflow-y:\s*auto;/,
    );
    expect(stylesheet()).toMatch(
      /\.ui-dialog\.composition-display-picker[\s\S]*?width:\s*min\(720px,\s*calc\(100vw - 32px\)\)/,
    );
  });

  it("uses a localized field ordinal without exposing a Form field ID", () => {
    render(() => (
      <CompositionDisplayPicker
        sources={[entrySource]}
        onAdd={() => {}}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog");
    fireEvent.click(within(dialog).getByRole("tab", { name: "Metric" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Tasks" }));

    const value = within(dialog).getByLabelText("Value");
    expect(value).toHaveDisplayValue("Field 1");
    expect(dialog).not.toHaveTextContent(entrySource.formId);
    expect(dialog).not.toHaveTextContent("100");
  });

  it("keeps metric field labels distinct when a Form name looks like a fallback", () => {
    const source: DraftSource = {
      ...entrySource,
      fieldSchema: [
        { field_id: 100, field_type: "string" },
        { field_id: 101, field_type: "string" },
        { field_id: 102, field_type: "string" },
      ],
    };
    render(() => (
      <CompositionDisplayPicker
        sources={[source]}
        fieldNames={(_formId, fieldId) =>
          fieldId === 100 ? "Field 2" : undefined}
        onAdd={() => {}}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog");
    fireEvent.click(within(dialog).getByRole("tab", { name: "Metric" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Tasks" }));
    const options = within(within(dialog).getByLabelText("Value"))
      .getAllByRole("option").map((option) => option.textContent);

    expect(options).toEqual(["Field 2", "Field 3", "Field 4"]);
    expect(new Set(options).size).toBe(options.length);
    expect(dialog).not.toHaveTextContent("100");
    expect(dialog).not.toHaveTextContent("101");
    expect(dialog).not.toHaveTextContent("102");
  });

  it("keeps compatible sources across kind tabs and clears metric values for Table", () => {
    render(() => (
      <CompositionDisplayPicker
        sources={[sqlSource, entrySource]}
        onAdd={() => {}}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog");
    const kindGroup = within(dialog).getByRole("tablist", {
      name: "Display type",
    });
    const monthly = within(dialog).getByRole("button", { name: "Monthly" });
    fireEvent.click(monthly);
    expect(monthly).toHaveAttribute("aria-pressed", "true");
    expect(within(dialog).queryByLabelText("Value")).toBeNull();
    expect(within(dialog).getByLabelText("Label")).toBeInTheDocument();

    const metricTab = within(kindGroup).getByRole("tab", { name: "Metric" });
    fireEvent.click(metricTab);
    expect(metricTab).toHaveAttribute("aria-selected", "true");
    expect(monthly).toHaveAttribute("aria-pressed", "true");
    expect(within(dialog).getByLabelText("Label")).toBeInTheDocument();
    expect(within(dialog).getByLabelText("Value")).toHaveDisplayValue("total");

    const tableTab = within(kindGroup).getByRole("tab", { name: "Table" });
    fireEvent.keyDown(metricTab, { key: "ArrowLeft" });
    expect(tableTab).toHaveAttribute("aria-selected", "true");
    expect(tableTab).toHaveFocus();
    expect(within(dialog).queryByLabelText("Value")).toBeNull();
    expect(within(dialog).getByLabelText("Label")).toBeInTheDocument();

    fireEvent.keyDown(tableTab, { key: "ArrowRight" });
    expect(metricTab).toHaveFocus();
    expect(metricTab).toHaveAttribute("aria-selected", "true");
    expect(monthly).toHaveAttribute("aria-pressed", "true");
    expect(within(dialog).getByLabelText("Value")).toHaveDisplayValue("total");
    fireEvent.keyDown(metricTab, { key: "Tab" });
    expect(within(dialog).getByRole("button", { name: "Monthly" }))
      .toHaveFocus();
  });

  it("clears a Table source that cannot provide a metric value", () => {
    render(() => (
      <CompositionDisplayPicker
        sources={[jsonOnlySource]}
        onAdd={() => {}}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog");
    const source = within(dialog).getByRole("button", { name: "Blobs" });
    fireEvent.click(source);
    expect(source).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(within(dialog).getByRole("tab", { name: "Metric" }));
    expect(within(dialog).getByRole("button", { name: "Blobs" }))
      .toBeDisabled();
    expect(within(dialog).queryByLabelText("Label")).toBeNull();
  });
});
