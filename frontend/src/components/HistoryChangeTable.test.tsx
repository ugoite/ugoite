import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor, within } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { HistoryChangeTable } from "./HistoryChangeTable";
import type { SpaceChangeQueryRow } from "~/lib/ugoite-client";

const row = (id: string): SpaceChangeQueryRow => ({
  change_id: id,
  generation: 1,
  change: {
    actor_principal_id: "human:editor",
    message: null,
    reverts_change_id: null,
    run_id: null,
    created_at_micros: 1767225600000000,
  },
  publication: {
    generation: 1,
    publication_uri: { space_uid: "space-1", key: "publication/1" },
    publication_checksum: "a".repeat(64),
  },
  target_visibility: "complete",
  summary: {
    affected_entry_count: 1,
    target_form_ids: ["form-1"],
    field_groups: [],
  },
});

const columns = [
  {
    key: "target",
    label: "Target",
    cell: (change: SpaceChangeQueryRow) => <span>{change.change_id}</span>,
  },
  {
    key: "date",
    label: "Date",
    cell: () => <span>2026-01-01</span>,
  },
];

const renderTable = (overrides?: {
  count?: number;
  selectedChangeId?: string;
}) => {
  const onSelectChange = vi.fn();
  const onOpenChange = vi.fn();
  render(() => (
    <HistoryChangeTable
      columns={columns}
      rows={[row("change-1"), row("change-2")].slice(0, overrides?.count ?? 2)}
      pageIdentity="history-page-1"
      tableLabel="Change pages"
      selectedChangeId={overrides?.selectedChangeId}
      onSelectChange={onSelectChange}
      openLabel="Open change"
      busy={false}
      onOpenChange={onOpenChange}
    />
  ));
  return { onSelectChange, onOpenChange };
};

describe("HistoryChangeTable", () => {
  it("renders the current Change page as a native table", () => {
    renderTable();
    expect(screen.getByRole("table")).toBeInTheDocument();
    expect(screen.getAllByRole("row")).toHaveLength(3);
    expect(screen.getAllByRole("columnheader").map((header) =>
      header.textContent
    )).toEqual(["Target", "Date", "Open change"]);
    expect(
      document.querySelector('[data-change-id="change-1"]'),
    ).not.toBeNull();
    expect(screen.queryByRole("navigation")).not.toBeInTheDocument();
  });

  it("selects on row click and opens only from the trailing control", () => {
    const { onSelectChange, onOpenChange } = renderTable();
    const change = screen.getByRole("row", { name: /change-1/ });
    fireEvent.click(change);
    expect(onSelectChange).toHaveBeenCalledTimes(1);
    expect(onOpenChange).not.toHaveBeenCalled();

    fireEvent.click(
      within(change).getByRole("button", { name: "Open change" }),
    );
    expect(onOpenChange).toHaveBeenCalledTimes(1);
    expect(onSelectChange).toHaveBeenCalledTimes(1);
  });

  it("marks the selected Change without recovery controls", () => {
    renderTable({ selectedChangeId: "change-2" });
    expect(screen.getByRole("row", { name: /change-2/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.queryByRole("button", { name: /revert/i })).toBeNull();
    expect(screen.queryByRole("button", { name: /undo/i })).toBeNull();
  });

  it("keeps the open action in a sticky trailing cell", () => {
    renderTable({ count: 1 });
    const change = screen.getByRole("row", { name: /change-1/ });
    const action = within(change).getByRole("button", { name: "Open change" });
    expect(action.closest("td")).toHaveClass("history-change-trailing-cell");
    expect(screen.getByRole("columnheader", { name: "Open change" }))
      .toHaveClass("history-change-trailing-header");
  });

  it("moves keyboard focus between row actions", async () => {
    renderTable();
    const actions = screen.getAllByRole("button", { name: "Open change" });
    expect(actions).toHaveLength(2);
    fireEvent.keyDown(actions[0], { key: "ArrowDown" });
    await waitFor(() => expect(actions[1]).toHaveFocus());
    fireEvent.keyDown(actions[1], { key: "ArrowUp" });
    await waitFor(() => expect(actions[0]).toHaveFocus());
  });
});
