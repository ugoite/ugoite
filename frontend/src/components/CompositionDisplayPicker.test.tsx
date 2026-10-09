import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  cleanup,
  fireEvent,
  render,
  screen,
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
    { field_id: 102, field_type: "binary" },
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

const rowReferenceOnlySource: DraftSource = {
  ...entrySource,
  draftId: "src-row-reference",
  name: "Relations",
  fieldSchema: [{ field_id: 102, field_type: "row_reference" }],
};

const stylesheet = () => readFileSync(join(__dirname, "..", "app.css"), "utf8");

describe("CompositionDisplayPicker", () => {
  beforeEach(() => setLocale("en"));
  afterEach(() => cleanup());

  it("adds a table as soon as an existing source is selected", () => {
    const onAdd = vi.fn();
    render(() => (
      <CompositionDisplayPicker
        sources={[sqlSource, entrySource]}
        onAdd={onAdd}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    const tabs = within(dialog).getByRole("tablist", {
      name: "Data component type",
    });
    expect(within(tabs).getByRole("tab", { name: "Table" }))
      .toHaveAttribute("aria-selected", "true");
    fireEvent.click(within(dialog).getByRole("button", { name: "Monthly" }));

    expect(onAdd).toHaveBeenCalledOnce();
    expect(onAdd).toHaveBeenCalledWith({
      kind: "table",
      sourceDraftId: sqlSource.draftId,
    });
    expect(dialog).not.toHaveTextContent(sqlSource.draftId);
    expect(dialog).not.toHaveTextContent(sqlSource.entryId);
    expect(dialog).not.toHaveTextContent(sqlSource.revisionId);
    expect(within(dialog).queryByLabelText("Label")).toBeNull();
    expect(within(dialog).queryByRole("button", { name: "Add" })).toBeNull();
  });

  it("adds a single-candidate metric on source selection and hides JSON fields", () => {
    const onAdd = vi.fn();
    render(() => (
      <CompositionDisplayPicker
        sources={[jsonOnlySource, sqlSource]}
        onAdd={onAdd}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Metric" }));
    expect(within(dialog).getByRole("button", { name: "Blobs" }))
      .toBeDisabled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Monthly" }));

    expect(onAdd).toHaveBeenCalledWith({
      kind: "metric",
      sourceDraftId: sqlSource.draftId,
      valueField: { column: "total" },
    });
    expect(dialog).not.toHaveTextContent("payload");
  });

  it("selects EntryQuery metrics by their human Form field names", () => {
    const onAdd = vi.fn();
    render(() => (
      <CompositionDisplayPicker
        sources={[entrySource]}
        fieldNames={(_formId, fieldId) =>
          fieldId === 100
            ? "Title"
            : fieldId === 102
            ? "Attachment"
            : undefined}
        onAdd={onAdd}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Metric" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Tasks" }));
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Attachment, Tasks" }),
    );

    expect(onAdd).toHaveBeenCalledWith({
      kind: "metric",
      sourceDraftId: entrySource.draftId,
      valueField: { fieldId: 102 },
    });
    expect(dialog).not.toHaveTextContent(entrySource.formId);
    expect(dialog).not.toHaveTextContent("100");
    expect(dialog).not.toHaveTextContent("102");
  });

  it("omits Form fields the server marks as non-projectable", () => {
    const source: DraftSource = {
      ...entrySource,
      fieldSchema: [
        { field_id: 100, field_type: "string" },
        { field_id: 102, field_type: "binary" },
        { field_id: 103, field_type: "integer" },
      ],
    };
    render(() => (
      <CompositionDisplayPicker
        sources={[source]}
        fieldNames={(_formId, fieldId) =>
          fieldId === 100
            ? "Task title"
            : fieldId === 102
            ? "Private note"
            : fieldId === 103
            ? "Amount"
            : undefined}
        fieldProjectable={(_formId, fieldId) => fieldId !== 102}
        onAdd={() => {}}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Metric" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Tasks" }));

    expect(
      within(dialog).getByRole("button", {
        name: "Task title, Tasks",
      }),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByRole("button", {
        name: "Amount, Tasks",
      }),
    ).toBeInTheDocument();
    expect(
      within(dialog).queryByRole("button", {
        name: "Private note, Tasks",
      }),
    ).toBeNull();
    expect(dialog).not.toHaveTextContent("102");
  });

  it("adds the only projectable metric field when the source is selected", () => {
    const onAdd = vi.fn();
    const source: DraftSource = {
      ...entrySource,
      fieldSchema: [
        { field_id: 100, field_type: "string" },
        { field_id: 102, field_type: "binary" },
      ],
    };
    render(() => (
      <CompositionDisplayPicker
        sources={[source]}
        fieldNames={(_formId, fieldId) =>
          fieldId === 100 ? "Task title" : "File size"}
        fieldProjectable={(_formId, fieldId) => fieldId === 100}
        onAdd={onAdd}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Metric" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Tasks" }));

    expect(onAdd).toHaveBeenCalledWith({
      kind: "metric",
      sourceDraftId: source.draftId,
      valueField: { fieldId: 100 },
    });
    expect(dialog).not.toHaveTextContent("File size");
    expect(dialog).not.toHaveTextContent("102");
  });

  it("does not show field ordinals or IDs when Form names are unavailable", () => {
    render(() => (
      <CompositionDisplayPicker
        sources={[entrySource]}
        onAdd={() => {}}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Metric" }));
    const source = within(dialog).getByRole("button", { name: "Tasks" });
    expect(source).toBeDisabled();
    expect(source).toHaveTextContent("Form field names unavailable");
    expect(dialog).not.toHaveTextContent(entrySource.formId);
    expect(dialog).not.toHaveTextContent("100");
    expect(dialog).not.toHaveTextContent("Field 1");
  });

  it("only offers fields that fit the source projection limit", () => {
    const source: DraftSource = {
      ...entrySource,
      fieldSchema: Array.from({ length: 65 }, (_, index) => ({
        field_id: index + 1,
        field_type: "integer",
      })),
      query: {
        filters: [],
        sort: [],
        projection: {
          kind: "fields",
          fields: Array.from({ length: 63 }, (_, index) => index + 1),
        },
      },
    };
    render(() => (
      <CompositionDisplayPicker
        sources={[source]}
        fieldNames={(_formId, fieldId) => `Name ${fieldId}`}
        onAdd={() => {}}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Metric" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Tasks" }));
    const fields = within(dialog).getAllByRole("button").filter((button) =>
      button.getAttribute("aria-label")?.includes(", Tasks")
    );

    expect(fields).toHaveLength(64);
    expect(fields.at(-1)).toHaveTextContent("Name 64");
    expect(dialog).not.toHaveTextContent("Name 65");
  });

  it("routes catalog browsing with the selected data kind", () => {
    const onChooseSource = vi.fn();
    render(() => (
      <CompositionDisplayPicker
        sources={[]}
        onAdd={() => {}}
        onChooseSource={onChooseSource}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Metric" }));
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "Choose a Form or Saved SQL",
      }),
    );

    expect(onChooseSource).toHaveBeenCalledWith("metric");
  });

  it("keeps full source names and the dialog usable in a narrow viewport", () => {
    const longName = "Monthly source with a long human name ".repeat(4).trim();
    render(() => (
      <CompositionDisplayPicker
        sources={[{ ...sqlSource, name: longName }]}
        onAdd={() => {}}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    const source = within(dialog).getByRole("button", { name: longName });
    expect(source).toHaveAttribute("title", longName);
    expect(source.querySelector(".rowListName")).toHaveTextContent(longName);
    expect(source.outerHTML).not.toContain("src-1");
    expect(source.outerHTML).not.toContain("sql-1");
    expect(stylesheet()).toMatch(
      /\.rowListName\s*\{[^}]*flex:\s*1 1 auto;[^}]*min-width:\s*0;[^}]*overflow:\s*hidden;[^}]*text-overflow:\s*ellipsis;[^}]*white-space:\s*nowrap;/,
    );
    expect(stylesheet()).toMatch(
      /\.ui-dialog\.composition-display-picker[\s\S]*?max-height:\s*calc\(100dvh - 32px\);[\s\S]*?overflow-y:\s*auto;/,
    );
  });

  it("keeps kind tabs keyboard-operable", () => {
    render(() => (
      <CompositionDisplayPicker
        sources={[sqlSource]}
        onAdd={() => {}}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    const metric = within(dialog).getByRole("tab", { name: "Metric" });
    const table = within(dialog).getByRole("tab", { name: "Table" });
    fireEvent.keyDown(table, { key: "ArrowRight" });
    expect(metric).toHaveFocus();
    expect(metric).toHaveAttribute("aria-selected", "true");
    fireEvent.keyDown(metric, { key: "ArrowLeft" });
    expect(table).toHaveFocus();
    expect(table).toHaveAttribute("aria-selected", "true");
  });

  it("allows a non-scalar source in Table mode", () => {
    const onAdd = vi.fn();
    render(() => (
      <CompositionDisplayPicker
        sources={[rowReferenceOnlySource]}
        onAdd={onAdd}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    const source = within(dialog).getByRole("button", { name: "Relations" });
    expect(source).toBeEnabled();
    fireEvent.click(source);
    expect(onAdd).toHaveBeenCalledWith({
      kind: "table",
      sourceDraftId: rowReferenceOnlySource.draftId,
    });
  });
});
