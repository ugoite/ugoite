import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import type { DraftDisplay, DraftSource } from "~/lib/composition-draft";
import { CompositionDisplayList } from "./CompositionDisplayList";

const sqlSource: DraftSource = {
  kind: "saved_sql",
  draftId: "src-1",
  entryId: "sql-1",
  revisionId: "sql-rev-1",
  name: "Monthly",
  expectedResult: [{ name: "total", type: "float" }],
  variables: {},
};

const entrySource: DraftSource = {
  kind: "entry_query",
  draftId: "src-2",
  formId: "11111111-1111-4111-8111-111111111111",
  name: "Tasks",
  fieldSchema: [{ field_id: 3, field_type: "string" }],
  query: { filters: [], sort: [], projection: { kind: "preview" } },
};

const tableDisplay: DraftDisplay = {
  kind: "table",
  draftId: "disp-1",
  sourceDraftId: "src-1",
};

const metricDisplay: DraftDisplay = {
  kind: "metric",
  draftId: "disp-2",
  sourceDraftId: "src-1",
  valueField: { column: "total" },
};

const renderList = (displays: readonly DraftDisplay[] = [
  tableDisplay,
  metricDisplay,
]) => {
  const onRemove = vi.fn();
  const onMove = vi.fn();
  const onChangeLabel = vi.fn();
  render(() => (
    <CompositionDisplayList
      sources={[sqlSource, entrySource]}
      displays={displays}
      headingId="studio-display-heading"
      onRemove={onRemove}
      onMove={onMove}
      onChangeLabel={onChangeLabel}
    />
  ));
  return { onRemove, onMove, onChangeLabel };
};

describe("CompositionDisplayList", () => {
  beforeEach(() => {
    setLocale("en");
  });

  afterEach(() => cleanup());

  it("renders display rows with default names and kind labels", () => {
    renderList();

    // Default names: source name for tables, source plus value for metrics.
    expect(screen.getByRole("button", { name: "Monthly" }))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Monthly / total" }))
      .toBeInTheDocument();
    expect(screen.getByText("Table")).toBeInTheDocument();
    expect(screen.getByText("Metric")).toBeInTheDocument();
  });

  it("edits labels inline and reorders or removes displays", () => {
    const { onRemove, onMove, onChangeLabel } = renderList();

    expect(screen.getByRole("button", { name: "Move Monthly up" }))
      .toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Move Monthly / total down" }),
    ).toBeDisabled();

    // Full-row activation toggles the detail with the inline label input.
    fireEvent.click(screen.getByRole("button", { name: "Monthly" }));
    const labelInput = screen.getByLabelText("Label");
    expect(labelInput).toHaveValue("");
    fireEvent.input(labelInput, { target: { value: "Totals" } });
    expect(onChangeLabel).toHaveBeenCalledWith("disp-1", "Totals");

    fireEvent.click(
      screen.getByRole("button", { name: "Move Monthly / total up" }),
    );
    expect(onMove).toHaveBeenCalledWith("disp-2", "up");

    fireEvent.click(
      screen.getByRole("button", { name: "Remove Monthly / total" }),
    );
    expect(onRemove).toHaveBeenCalledWith("disp-2");
  });

  it("shows one next-action line when empty", () => {
    const { container } = render(() => (
      <CompositionDisplayList
        sources={[sqlSource]}
        displays={[]}
        headingId="studio-display-heading"
        onRemove={() => {}}
        onMove={() => {}}
        onChangeLabel={() => {}}
      />
    ));

    const paragraphs = container.querySelectorAll("p");
    expect(paragraphs).toHaveLength(1);
    expect(paragraphs[0]).toHaveTextContent("Add a display to begin.");
  });
});
