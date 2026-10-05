import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor, within } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { EntryResultTable } from "./EntryResultTable";
import type { EntryQueryResult } from "~/lib/entry-query";

const rows = (count: number): EntryQueryResult[] =>
  Array.from({ length: count }, (_, index) => ({
    id: `entry-${index}`,
    form_id: "form-1",
    revision_id: `revision-${index}`,
    created_at_micros: 1_772_960_000_000_000,
    updated_at_micros: 1_772_963_000_000_000,
    preview: `Entry ${index}`,
  }));

const columns = [{
  key: "preview",
  label: "Preview",
  cell: (row: EntryQueryResult) => <span>{row.preview}</span>,
}];

const renderTable = (overrides?: {
  count?: number;
  selectedEntryId?: string;
  trailingAction?: "open" | "confirm";
}) => {
  const onSelectEntry = vi.fn();
  const onOpenEntry = vi.fn();
  render(() => (
    <EntryResultTable
      columns={columns}
      rows={rows(overrides?.count ?? 2)}
      pageIdentity="entry-page-1"
      tableLabel="Entry pages"
      selectedEntryId={overrides?.selectedEntryId}
      onSelectEntry={onSelectEntry}
      trailingAction={overrides?.trailingAction ?? "open"}
      openLabel="Open entry"
      confirmLabel="Use this entry"
      busy={false}
      onOpenEntry={onOpenEntry}
    />
  ));
  return { onSelectEntry, onOpenEntry };
};

describe("EntryResultTable", () => {
  it("renders the current page as a native table with Entry identity", () => {
    renderTable();
    expect(screen.getByRole("table")).toBeInTheDocument();
    expect(screen.getAllByRole("row")).toHaveLength(3);
    expect(
      document.querySelector('[data-entry-id="entry-0"]'),
    ).not.toBeNull();
    expect(screen.queryByText("entry-0")).not.toBeInTheDocument();
    expect(screen.queryByRole("navigation")).not.toBeInTheDocument();
  });

  it("selects on row click and opens only from the trailing control", () => {
    const { onSelectEntry, onOpenEntry } = renderTable();
    const row = screen.getByRole("row", { name: /Entry 0/ });
    fireEvent.click(row);
    expect(onSelectEntry).toHaveBeenCalledTimes(1);
    expect(onOpenEntry).not.toHaveBeenCalled();

    fireEvent.click(
      within(row).getByRole("button", { name: "Open entry" }),
    );
    expect(onOpenEntry).toHaveBeenCalledTimes(1);
    expect(onSelectEntry).toHaveBeenCalledTimes(1);
  });

  it("marks the selected Entry without navigating", () => {
    renderTable({ selectedEntryId: "entry-1" });
    const row = screen.getByRole("row", { name: /Entry 1/ });
    expect(row).toHaveAttribute("aria-selected", "true");
    expect(
      screen.getByRole("row", { name: /Entry 0/ }),
    ).not.toHaveAttribute("aria-selected");
  });

  it("keeps the open action in a sticky trailing cell", () => {
    renderTable({ count: 1 });
    const row = screen.getByRole("row", { name: /Entry 0/ });
    const action = within(row).getByRole("button", { name: "Open entry" });
    expect(action.closest("td")).toHaveClass("entry-browser-trailing-cell");
    expect(screen.getByRole("columnheader", { name: "Open entry" }))
      .toHaveClass("entry-browser-trailing-header");
  });

  it("confirms from the trailing control in select mode", () => {
    const { onOpenEntry } = renderTable({
      trailingAction: "confirm",
      count: 1,
    });
    fireEvent.click(screen.getByRole("button", { name: "Use this entry" }));
    expect(onOpenEntry).toHaveBeenCalledTimes(1);
  });

  it("moves keyboard focus between row actions", async () => {
    renderTable();
    const actions = screen.getAllByRole("button", { name: "Open entry" });
    expect(actions).toHaveLength(2);
    fireEvent.keyDown(actions[0], { key: "ArrowDown" });
    await waitFor(() => expect(actions[1]).toHaveFocus());
    fireEvent.keyDown(actions[1], { key: "ArrowUp" });
    await waitFor(() => expect(actions[0]).toHaveFocus());
  });
});
