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
    { name: "count", type: "integer" },
    { name: "payload", type: "json" },
  ],
  variables: {},
};

const entrySource: DraftSource = {
  kind: "entry_query",
  draftId: "src-2",
  formId: "11111111-1111-4111-8111-111111111111",
  name: "Tasks",
  fieldSchema: [{ field_id: 100, field_type: "double" }],
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

const draftWithSources = (...sources: DraftSource[]) => ({
  ...createEmptyDraft("Studio"),
  sources,
});

describe("CompositionDisplayPicker", () => {
  beforeEach(() => setLocale("en"));
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("omits JSON and ambiguous Saved SQL result names", () => {
    expect(savedSqlScalarCandidates([
      { name: "total", type: "float" },
      { name: "total", type: "integer" },
      { name: "payload", type: "json" },
      { name: "count", type: "integer" },
    ])).toEqual([{ id: "count", name: "count" }]);
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

  it("keeps full source names available for selectable and unavailable rows", () => {
    const longName = "Monthly totals with a long name ".repeat(4).trim();
    const longUnavailableName = "Archived JSON data with a long name ".repeat(4)
      .trim();
    const longSource = { ...sqlSource, name: longName };
    const unavailableSource = {
      ...jsonOnlySource,
      name: longUnavailableName,
    };
    render(() => (
      <CompositionDisplayPicker
        draft={draftWithSources(longSource, unavailableSource)}
        initialKind="metric"
        onAdd={() => {}}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    const selectableRow = within(dialog).getByRole("button", {
      name: longName,
    });
    const unavailableRow = within(dialog).getByRole("button", {
      name: longUnavailableName,
    });
    expect(selectableRow).toBeEnabled();
    expect(selectableRow).toHaveAttribute("title", longName);
    expect(selectableRow.querySelector(".rowListName"))
      .toHaveTextContent(longName);
    expect(unavailableRow).toBeDisabled();
    expect(unavailableRow).toHaveAttribute("title", longUnavailableName);
    expect(unavailableRow.querySelector(".rowListName"))
      .toHaveTextContent(longUnavailableName);
    expect(dialog).not.toHaveTextContent(sqlSource.draftId);
    expect(dialog).not.toHaveTextContent(sqlSource.entryId);
    expect(dialog).not.toHaveTextContent(sqlSource.revisionId);
  });

  it("switches Table and Metric with arrow-key tabs", () => {
    render(() => (
      <CompositionDisplayPicker
        draft={draftWithSources(sqlSource, entrySource)}
        onAdd={() => {}}
        onClose={() => {}}
      />
    ));

    const tableTab = screen.getByRole("tab", { name: "Table" });
    const metricTab = screen.getByRole("tab", { name: "Metric" });
    fireEvent.keyDown(tableTab, { key: "ArrowRight" });

    expect(metricTab).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByRole("button", { name: "Tasks" }))
      .not.toBeInTheDocument();

    fireEvent.keyDown(metricTab, { key: "ArrowLeft" });
    expect(tableTab).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("button", { name: "Tasks" }))
      .toBeInTheDocument();
  });

  it("previews a selected Saved SQL column before inserting a Metric", async () => {
    const onAdd = vi.fn();
    const onPreview = vi.fn().mockResolvedValue({
      ok: true,
      value: "9007199254740993.125",
    });
    render(() => (
      <CompositionDisplayPicker
        draft={draftWithSources(sqlSource, entrySource, jsonOnlySource)}
        initialKind="metric"
        onAdd={onAdd}
        onPreview={onPreview}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    expect(within(dialog).queryByRole("button", { name: "Tasks" }))
      .not.toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Blobs" }))
      .toBeDisabled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Monthly" }));
    expect(within(dialog).getByLabelText("Column")).toHaveValue("");
    expect(within(dialog).getByRole("button", { name: "Add" }))
      .toBeDisabled();

    fireEvent.change(within(dialog).getByLabelText("Column"), {
      target: { value: "total" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Preview" }));

    expect(await within(dialog).findByText("9007199254740993.125"))
      .toBeInTheDocument();
    expect(onPreview).toHaveBeenCalledWith({
      sourceDraftId: sqlSource.draftId,
      column: "total",
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Add" }));
    expect(onAdd).toHaveBeenCalledWith({
      kind: "metric",
      sourceDraftId: sqlSource.draftId,
      column: "total",
    });
    expect(dialog).not.toHaveTextContent("payload");
    expect(dialog).not.toHaveTextContent("sql-rev-1");
  });

  it("keeps a successful NULL distinct from zero in the preview", async () => {
    const onPreview = vi.fn().mockResolvedValue({ ok: true, value: null });
    render(() => (
      <CompositionDisplayPicker
        draft={draftWithSources(sqlSource)}
        initialKind="metric"
        onAdd={() => {}}
        onPreview={onPreview}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Monthly" }));
    fireEvent.change(within(dialog).getByLabelText("Column"), {
      target: { value: "total" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Preview" }));

    expect(await within(dialog).findByLabelText("No data"))
      .toHaveTextContent("—");
    expect(within(dialog).getByRole("button", { name: "Add" }))
      .toBeEnabled();
  });

  it("discards a late preview after the selected column changes", async () => {
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
        draft={draftWithSources(sqlSource)}
        initialKind="metric"
        onAdd={() => {}}
        onPreview={onPreview}
        onClose={() => {}}
      />
    ));

    const dialog = screen.getByRole("dialog", { name: "Add data component" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Monthly" }));
    const column = within(dialog).getByLabelText("Column");
    fireEvent.change(column, { target: { value: "total" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Preview" }));
    fireEvent.change(column, { target: { value: "count" } });
    resolvePreview?.({ ok: true, value: "stale" });

    await vi.waitFor(() => expect(onPreview).toHaveBeenCalledTimes(1));
    expect(within(dialog).queryByText("stale")).not.toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Add" }))
      .toBeDisabled();
  });
});
