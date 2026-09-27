import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor, within } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { changeApi, formApi, spaceApi } from "~/lib/ugoite-client";
import SpaceHistoryRoute from "./history";

vi.mock("@solidjs/router", () => ({ useParams: () => ({ space_id: "default" }) }));
vi.mock("~/lib/ugoite-client", () => ({
  changeApi: { query: vi.fn(), revert: vi.fn(), undoRun: vi.fn() },
  formApi: { list: vi.fn() },
  spaceApi: { listMembers: vi.fn() },
}));

const row = (id: string, count: number, runId: string | null = null) => ({
  change_id: id,
  generation: count,
  change: {
    actor_principal_id: "human:editor",
    message: null,
    reverts_change_id: null,
    run_id: runId,
    created_at_micros: 1767225600000000,
  },
  publication: {
    generation: count,
    publication_uri: { space_uid: "space-1", key: `publication/${count}` },
    publication_checksum: "a".repeat(64),
  },
  target_visibility: "complete" as const,
  summary: {
    affected_entry_count: count,
    field_groups: [{
      form_id: "form-1",
      field_id: "1",
      before: { state: "value" as const, value: "Travel" },
      after: { state: "value" as const, value: "Business travel" },
      affected_entry_count: count,
    }],
  },
});

describe("space history list", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    setLocale("en");
    vi.mocked(formApi.list).mockResolvedValue([{
      id: "form-1", name: "Expenses", version: 1, template: "", fields: { purpose: { id: 1, type: "string", required: false } },
    }]);
    vi.mocked(spaceApi.listMembers).mockResolvedValue([{
      principal: { principal_id: "human:editor", display_name: "Ada Example", kind: "human", state: "active" },
      role: "owner",
    }]);
    vi.mocked(changeApi.query).mockResolvedValue({
      changes: [row("change-1", 100, "run-1"), row("change-2", 40, "run-1")],
      next_cursor: null,
    });
    vi.mocked(changeApi.revert).mockResolvedValue({
      change_id: "inverse-change",
      reverts_change_id: "change-1",
      run_id: null,
    });
    vi.mocked(changeApi.undoRun).mockResolvedValue({
      run_id: "run-1",
      reverted_change_count: 2,
    });
  });

  it("shows one bounded row per Change with safe, evidence-backed summaries", async () => {
    render(() => <SpaceHistoryRoute />);

    expect(await screen.findByText("Expenses · 100 entries")).toBeInTheDocument();
    expect(screen.getByText("purpose: Travel → Business travel (100)")).toBeInTheDocument();
    expect(within(document.querySelector("tbody")!).getAllByText("Ada Example")).toHaveLength(2);
    expect(document.querySelectorAll("tbody tr")).toHaveLength(2);
    expect(screen.queryByText("change-1")).toBeNull();
    expect(screen.queryByText("run-1")).toBeNull();
    expect(changeApi.query).toHaveBeenCalledWith("default", expect.objectContaining({ limit: 50 }));
    fireEvent.click(screen.getAllByRole("button", { name: "Open change" })[0]);
    expect(document.querySelector("tbody tr[aria-selected='true']")).toBeInTheDocument();
    expect(await screen.findByRole("dialog", { name: "Expenses · 100 entries" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("searchbox", { name: "Search history" })).toHaveValue("");
  });

  it("keeps the existing Change revert flow available from the Change detail", async () => {
    render(() => <SpaceHistoryRoute />);
    fireEvent.click((await screen.findAllByRole("button", { name: "Open change" }))[0]);
    fireEvent.click(await screen.findByRole("button", { name: "Revert this change" }));
    expect(await screen.findByText(/appends a new Change/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Append new Change" }));
    expect(await screen.findByText("Reverted as Change inverse-change.")).toBeInTheDocument();
    expect(changeApi.revert).toHaveBeenCalledWith("default", "change-1", {});
    expect(changeApi.query).toHaveBeenCalledTimes(2);
  });

  it("traps focus in the detail dialog and restores it when closed with Escape", async () => {
    render(() => <SpaceHistoryRoute />);
    const opener = (await screen.findAllByRole("button", { name: "Open change" }))[0];
    fireEvent.click(opener);
    const dialog = await screen.findByRole("dialog", { name: "Expenses · 100 entries" });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Back" }));
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Undo run" }));
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(opener));
  });

  it("blocks another recovery attempt until an unknown result is reconciled", async () => {
    let calls = 0;
    vi.mocked(changeApi.query).mockImplementation(async () => {
      calls += 1;
      if (calls === 2) throw new Error("offline");
      return { changes: [row("change-1", 100, "run-1")], next_cursor: null };
    });
    vi.mocked(changeApi.revert).mockRejectedValue(new Error("connection lost"));
    render(() => <SpaceHistoryRoute />);
    fireEvent.click((await screen.findAllByRole("button", { name: "Open change" }))[0]);
    fireEvent.click(await screen.findByRole("button", { name: "Revert this change" }));
    fireEvent.click(await screen.findByRole("button", { name: "Append new Change" }));
    expect(await screen.findByRole("button", { name: "Retry history refresh" })).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).toBeNull();
    vi.mocked(changeApi.revert).mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Retry history refresh" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Retry history refresh" })).toBeNull());
    expect(changeApi.revert).not.toHaveBeenCalled();
    expect(screen.getByText("The recovery was not recorded in history. You can try again.")).toBeInTheDocument();
  });

  it("reconciles an uncertain Change revert without the active list filters", async () => {
    const original = row("change-1", 1);
    const inverse = {
      ...row("inverse-change", 2),
      change: {
        ...row("inverse-change", 2).change,
        actor_principal_id: "human:owner",
        reverts_change_id: "change-1",
      },
    };
    const requests: Array<Record<string, unknown>> = [];
    vi.mocked(changeApi.query).mockImplementation(async (_spaceId, filters = {}) => {
      const request = filters as Record<string, unknown>;
      requests.push(request);
      if (request.actor_principal_id) {
        return { changes: [original], next_cursor: null };
      }
      if (requests.length === 3) {
        return { changes: [inverse, original], next_cursor: null };
      }
      return { changes: [original], next_cursor: null };
    });
    vi.mocked(changeApi.revert).mockRejectedValue(new Error("connection lost"));
    render(() => <SpaceHistoryRoute />);
    await screen.findByText("Expenses · 1 entries");
    fireEvent.click(screen.getByText("Columns, filters, and sort"));
    fireEvent.change(screen.getByLabelText("Filter by actor"), { target: { value: "human:editor" } });
    await waitFor(() => expect(changeApi.query).toHaveBeenLastCalledWith("default", expect.objectContaining({ actor_principal_id: "human:editor" })));
    fireEvent.click((await screen.findAllByRole("button", { name: "Open change" }))[0]);
    fireEvent.click(await screen.findByRole("button", { name: "Revert this change" }));
    fireEvent.click(await screen.findByRole("button", { name: "Append new Change" }));
    expect(await screen.findByText("The recovery is confirmed in history.")).toBeInTheDocument();
    expect(requests[2]).toEqual({ limit: 50, cursor: undefined });
    expect(screen.queryByRole("button", { name: "Retry history refresh" })).toBeNull();
  });

  it("hides target counts and field groups when target visibility is partial", async () => {
    vi.mocked(changeApi.query).mockResolvedValue({
      changes: [{ ...row("hidden-change", 100), target_visibility: "partial", summary: null }],
      next_cursor: null,
    });
    render(() => <SpaceHistoryRoute />);
    expect(await screen.findByText("Restricted target")).toBeInTheDocument();
    expect(screen.getByText("Some change details are unavailable")).toBeInTheDocument();
    expect(screen.queryByText(/100 entries/)).toBeNull();
    expect(screen.queryByText(/purpose:/)).toBeNull();
  });

  it("applies supported text, actor, date, sort, and column controls server-side", async () => {
    render(() => <SpaceHistoryRoute />);
    await screen.findByText("Expenses · 100 entries");
    fireEvent.input(screen.getByRole("searchbox", { name: "Search history" }), { target: { value: "travel" } });
    await waitFor(() => expect(changeApi.query).toHaveBeenLastCalledWith("default", expect.objectContaining({ text: "travel", limit: 50 })));
    fireEvent.click(screen.getByText("Columns, filters, and sort"));
    const actorFilter = screen.getByLabelText("Filter by actor");
    fireEvent.change(actorFilter, { target: { value: "human:editor" } });
    fireEvent.input(screen.getByLabelText("From date"), { target: { value: "2026-01-01" } });
    fireEvent.input(screen.getByLabelText("To date"), { target: { value: "2026-01-02" } });
    const dateSort = screen.getByLabelText("Sort by date");
    fireEvent.change(dateSort, { target: { value: "asc" } });
    const actorSort = screen.getByLabelText("Sort by actor");
    fireEvent.change(actorSort, { target: { value: "desc" } });
    await waitFor(() => expect(changeApi.query).toHaveBeenLastCalledWith("default", expect.objectContaining({
      actor_principal_id: "human:editor",
      created_after_micros: Date.parse("2026-01-01T00:00:00.000Z") * 1000,
      created_before_micros: Date.parse("2026-01-02T23:59:59.999Z") * 1000 + 999,
      sort: [
        { field: "created_at_micros", direction: "asc" },
        { field: "actor_principal_id", direction: "desc" },
      ],
    })));
  });
});
