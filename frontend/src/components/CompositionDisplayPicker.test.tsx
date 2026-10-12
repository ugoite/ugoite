import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { createEmptyDraft, type DraftSource } from "~/lib/composition-draft";
const { compositionAggregateCapabilityMock } = vi.hoisted(() => ({
  compositionAggregateCapabilityMock: vi.fn(),
}));
vi.mock("~/lib/ugoite-client", () => ({
  compositionAggregateCapability: compositionAggregateCapabilityMock,
}));
import {
  CompositionDisplayPicker,
  savedSqlScalarCandidates,
} from "./CompositionDisplayPicker";

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
    { field_id: 101, field_type: "double" },
  ],
  query: {
    filters: [],
    sort: [],
    projection: { kind: "preview" },
  },
};

const jsonOnlySource: DraftSource = {
  ...sqlSource,
  draftId: "src-3",
  name: "Blobs",
  expectedResult: [{ name: "payload", type: "json" }],
};

const rowReferenceOnlySource: DraftSource = {
  ...entrySource,
  draftId: "src-4",
  name: "Relations",
  fieldSchema: [{ field_id: 102, field_type: "row_reference" }],
};

const draftWithSources = (...sources: DraftSource[]) => ({
  ...createEmptyDraft("Studio"),
  sources,
});

describe("CompositionDisplayPicker", () => {
  beforeEach(() => {
    setLocale("en");
    compositionAggregateCapabilityMock.mockImplementation(
      async (operation: string, fieldType?: string) => {
        const supported = operation === "count_values" ||
          operation === "min" &&
            (fieldType === "double" || fieldType === "date") ||
          operation === "sum" && fieldType === "double";
        return supported
          ? { supported: true, result_type: "integer" }
          : { supported: false };
      },
    );
  });
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("omits ambiguous Saved SQL result names", () => {
    expect(savedSqlScalarCandidates([
      { name: "total", type: "float" },
      { name: "total", type: "integer" },
      { name: "payload", type: "json" },
    ])).toEqual([]);
  });

  it("adds a table immediately when a human-named source is selected", () => {
    const onAdd = vi.fn();
    render(() => (
      <CompositionDisplayPicker
        draft={draftWithSources(sqlSource, entrySource)}
        onAdd={onAdd}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Monthly" }));

    expect(onAdd).toHaveBeenCalledWith({
      kind: "table",
      sourceDraftId: sqlSource.draftId,
    });
    expect(dialog).not.toHaveTextContent(sqlSource.draftId);
    expect(dialog).not.toHaveTextContent(sqlSource.entryId);
    expect(dialog).not.toHaveTextContent(sqlSource.revisionId);
  });

  it("previews an exact Saved SQL scalar before inserting it", async () => {
    const onAdd = vi.fn();
    const onPreview = vi.fn().mockResolvedValue({
      ok: true,
      value: "9007199254740993.125",
    });
    render(() => (
      <CompositionDisplayPicker
        draft={draftWithSources(sqlSource, jsonOnlySource)}
        initialKind="metric"
        onAdd={onAdd}
        onPreview={onPreview}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    expect(within(dialog).getByRole("button", { name: "Blobs" }))
      .toBeDisabled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Monthly" }));
    expect(within(dialog).getByLabelText("Column")).toHaveValue("");
    expect(onAdd).not.toHaveBeenCalled();
    fireEvent.change(within(dialog).getByLabelText("Column"), {
      target: { value: "total" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Preview" }));

    expect(await within(dialog).findByText("9007199254740993.125"))
      .toBeInTheDocument();
    expect(onPreview).toHaveBeenCalledWith({
      sourceDraftId: sqlSource.draftId,
      definition: { kind: "sql_scalar", column: "total" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Add" }));
    expect(onAdd).toHaveBeenCalledWith({
      kind: "metric",
      sourceDraftId: sqlSource.draftId,
      definition: { kind: "sql_scalar", column: "total" },
    });
    expect(dialog).not.toHaveTextContent("payload");
    expect(dialog).not.toHaveTextContent("sql-rev-1");
  });

  it("configures Form sum by operation and human field name before insertion", async () => {
    const onAdd = vi.fn();
    const onPreview = vi.fn().mockResolvedValue({ ok: true, value: "42" });
    render(() => (
      <CompositionDisplayPicker
        draft={draftWithSources(entrySource)}
        initialKind="metric"
        fieldNames={(_formId, fieldId) =>
          fieldId === 100
            ? "Task title"
            : fieldId === 101
            ? "Amount"
            : undefined}
        onAdd={onAdd}
        onPreview={onPreview}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Tasks" }));
    expect(within(dialog).getByLabelText("Operation")).toHaveValue(
      "count_rows",
    );
    fireEvent.change(within(dialog).getByLabelText("Operation"), {
      target: { value: "sum" },
    });
    await vi.waitFor(() =>
      expect(compositionAggregateCapabilityMock).toHaveBeenCalled()
    );
    expect(compositionAggregateCapabilityMock.mock.calls).toEqual([
      ["sum", "string"],
      ["sum", "double"],
    ]);
    await within(dialog).findByRole("option", { name: "Amount" });
    expect(compositionAggregateCapabilityMock).toHaveBeenCalledWith(
      "sum",
      "string",
    );
    expect(compositionAggregateCapabilityMock).toHaveBeenCalledWith(
      "sum",
      "double",
    );
    const field = within(dialog).getByLabelText("Field");
    expect(field).toHaveValue("");
    expect(
      Array.from(field.querySelectorAll("option")).map((entry) =>
        entry.textContent
      ),
    ).toEqual(["Choose a field", "Amount"]);
    expect(within(dialog).getByRole("button", { name: "Preview" }))
      .toBeDisabled();
    fireEvent.change(field, { target: { value: "101" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Preview" }));
    expect(await within(dialog).findByText("42")).toBeInTheDocument();
    expect(onPreview).toHaveBeenCalledWith({
      sourceDraftId: entrySource.draftId,
      definition: { kind: "aggregate", operation: "sum", fieldId: 101 },
    });
    expect(dialog).not.toHaveTextContent(entrySource.formId);
    expect(dialog).not.toHaveTextContent("100");
    expect(dialog).not.toHaveTextContent("101");
    fireEvent.click(within(dialog).getByRole("button", { name: "Add" }));
    expect(onAdd).toHaveBeenCalledWith({
      kind: "metric",
      sourceDraftId: entrySource.draftId,
      definition: { kind: "aggregate", operation: "sum", fieldId: 101 },
    });
  });

  it("uses the Rust capability contract for named Form field choices", async () => {
    const source: DraftSource = {
      ...entrySource,
      fieldSchema: [
        { field_id: 100, field_type: "string" },
        { field_id: 101, field_type: "double" },
        { field_id: 102, field_type: "date" },
        { field_id: 103, field_type: "row_reference" },
        { field_id: 104, field_type: "list" },
      ],
    };
    const names = new Map([
      [100, "Task title"],
      [101, "Amount"],
      [102, "Started at"],
      [103, "Parent task"],
      [104, "Steps"],
    ]);
    render(() => (
      <CompositionDisplayPicker
        draft={draftWithSources(source)}
        initialKind="metric"
        fieldNames={(_formId, fieldId) => names.get(fieldId)}
        onAdd={() => {}}
        onPreview={async () => ({ ok: true, value: "1" })}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Tasks" }));
    const operation = within(dialog).getByLabelText("Operation");
    fireEvent.change(operation, { target: { value: "min" } });
    const field = within(dialog).getByLabelText("Field");
    await within(dialog).findByRole("option", { name: "Started at" });
    expect(compositionAggregateCapabilityMock).toHaveBeenCalledWith(
      "min",
      "row_reference",
    );
    expect(
      Array.from(field.querySelectorAll("option")).map((option) =>
        option.textContent
      ),
    ).toEqual(["Choose a field", "Amount", "Started at"]);
    expect(dialog).not.toHaveTextContent("103");
    expect(dialog).not.toHaveTextContent("104");

    fireEvent.change(operation, { target: { value: "count_values" } });
    await within(dialog).findByRole("option", { name: "Steps" });
    expect(compositionAggregateCapabilityMock).toHaveBeenCalledWith(
      "count_values",
      "list",
    );
    expect(
      Array.from(field.querySelectorAll("option")).map((option) =>
        option.textContent
      ),
    ).toEqual([
      "Choose a field",
      "Task title",
      "Amount",
      "Started at",
      "Parent task",
      "Steps",
    ]);
  });

  it("keeps successful NULL distinct from zero in the preview", async () => {
    const onPreview = vi.fn().mockResolvedValue({ ok: true, value: null });
    render(() => (
      <CompositionDisplayPicker
        draft={draftWithSources(entrySource)}
        initialKind="metric"
        onAdd={() => {}}
        onPreview={onPreview}
        onClose={() => {}}
      />
    ));
    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Tasks" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Preview" }));
    const noData = await within(dialog).findByLabelText("No data");
    expect(noData).toHaveTextContent("—");
    expect(within(dialog).getByRole("button", { name: "Add" })).toBeEnabled();
  });

  it("discards a late preview when the Metric definition changes", async () => {
    let resolvePreview:
      | ((result: { ok: boolean; value?: unknown; error?: string }) => void)
      | undefined;
    const onPreview = vi.fn(() =>
      new Promise<{ ok: boolean; value?: unknown; error?: string }>(
        (resolve) => resolvePreview = resolve,
      )
    );
    render(() => (
      <CompositionDisplayPicker
        draft={draftWithSources(entrySource)}
        initialKind="metric"
        fieldNames={(_formId, fieldId) =>
          fieldId === 101 ? "Amount" : undefined}
        onAdd={() => {}}
        onPreview={onPreview}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Tasks" }));
    const operation = within(dialog).getByLabelText("Operation");
    fireEvent.change(operation, { target: { value: "sum" } });
    await within(dialog).findByRole("option", { name: "Amount" });
    fireEvent.change(within(dialog).getByLabelText("Field"), {
      target: { value: "101" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Preview" }));
    expect(onPreview).toHaveBeenCalledTimes(1);

    fireEvent.change(operation, { target: { value: "count_values" } });
    expect(resolvePreview).toBeDefined();
    resolvePreview?.({ ok: true, value: "42" });
    await Promise.resolve();
    await Promise.resolve();

    expect(within(dialog).queryByText("42")).not.toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Add" }))
      .toBeDisabled();
  });

  it("keeps Forms selectable when names are unavailable without showing IDs", () => {
    render(() => (
      <CompositionDisplayPicker
        draft={draftWithSources(entrySource)}
        initialKind="metric"
        onAdd={() => {}}
        onPreview={async () => ({ ok: true, value: "1" })}
        onClose={() => {}}
      />
    ));
    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    const source = within(dialog).getByRole("button", { name: "Tasks" });
    expect(source).toBeEnabled();
    fireEvent.click(source);
    fireEvent.change(within(dialog).getByLabelText("Operation"), {
      target: { value: "sum" },
    });
    expect(within(dialog).getByLabelText("Field")).toHaveValue("");
    expect(within(dialog).getByRole("button", { name: "Preview" }))
      .toBeDisabled();
    expect(dialog).not.toHaveTextContent(entrySource.formId);
    expect(dialog).not.toHaveTextContent("100");
    expect(dialog).not.toHaveTextContent("101");
    expect(dialog).not.toHaveTextContent("Field 1");
  });

  it("routes catalog browsing with the selected data kind", () => {
    const onChooseSource = vi.fn();
    render(() => (
      <CompositionDisplayPicker
        draft={createEmptyDraft()}
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

  it("keeps kind tabs keyboard-operable and permits non-scalar Tables", () => {
    const onAdd = vi.fn();
    render(() => (
      <CompositionDisplayPicker
        draft={draftWithSources(rowReferenceOnlySource)}
        onAdd={onAdd}
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
    fireEvent.click(within(dialog).getByRole("button", { name: "Relations" }));
    expect(onAdd).toHaveBeenCalledWith({
      kind: "table",
      sourceDraftId: rowReferenceOnlySource.draftId,
    });
  });
});
