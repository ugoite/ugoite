import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { changeApi, formApi, spaceApi } from "~/lib/ugoite-client";
import SpaceHistoryRoute from "./history";

const searchParams = vi.hoisted(() => ({ value: {} as Record<string, string> }));
const locationState = vi.hoisted(() => ({ value: null as { historyRecoveryNotice?: unknown } | null }));
const setSearchParams = vi.hoisted(() => vi.fn((
  value: Record<string, string | undefined>,
  options?: { state?: unknown },
) => {
  for (const [key, item] of Object.entries(value)) {
    if (item === undefined) delete searchParams.value[key];
    else searchParams.value[key] = item;
  }
  if (options?.state && typeof options.state === "object") {
    locationState.value = options.state as typeof locationState.value;
  }
}));
vi.mock("@solidjs/router", () => ({
  useParams: () => ({ space_id: "default" }),
  useLocation: () => ({ state: locationState.value }),
  useSearchParams: () => [searchParams.value, setSearchParams],
}));
vi.mock("~/lib/ugoite-client", () => ({
  changeApi: { query: vi.fn(), inspect: vi.fn(), affectedEntry: vi.fn(), revert: vi.fn(), undoRun: vi.fn() },
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
    target_form_ids: ["form-1"],
    field_groups: [{
      form_id: "form-1",
      field_id: 1,
      before: { state: "value" as const, value: "Travel" },
      after: { state: "value" as const, value: "Business travel" },
      affected_entry_count: count,
    }],
  },
});

describe("space history list", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    searchParams.value = {};
    locationState.value = null;
    setSearchParams.mockImplementation((value, options) => {
      for (const [key, item] of Object.entries(value)) {
        if (item === undefined) delete searchParams.value[key];
        else searchParams.value[key] = item;
      }
      if (options?.state && typeof options.state === "object") {
        locationState.value = options.state as typeof locationState.value;
      }
    });
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
    vi.mocked(changeApi.inspect).mockImplementation(async (_spaceId, changeId) => {
      const source = row(changeId, 100, "run-1");
      return {
        change_id: changeId,
        change: source.change,
        target_visibility: "complete",
        summary: source.summary,
        targets: [{ form_id: "form-1", entry_id: "entry-1", before_revision_id: "rev-0", after_revision_id: "rev-1", operation: "update", fields: [{ field_id: 1, before: { state: "value", value: "Travel" }, after: { state: "value", value: "Business travel" } }] }],
        next_cursor: null,
      };
    });
    vi.mocked(changeApi.affectedEntry).mockResolvedValue({
      form_id: "form-1", entry_id: "entry-1", before_revision_id: "rev-0", after_revision_id: "rev-1", operation: "update",
      fields: [{ field_id: 1, before: { state: "value", value: "Travel" }, after: { state: "value", value: "Business travel" } }],
    });
  });

  it("shows one bounded row per Change with safe, evidence-backed summaries", async () => {
    render(() => <SpaceHistoryRoute />);

    expect(await screen.findByText("Expenses · 100 entries")).toBeInTheDocument();
    expect(screen.getByText("purpose: Travel → Business travel (100)")).toBeInTheDocument();
    expect(within(document.querySelector("tbody")!).getAllByText("Ada Example")).toHaveLength(2);
    expect(screen.queryByText("human:editor")).toBeNull();
    expect(document.querySelectorAll("tbody tr")).toHaveLength(2);
    expect(screen.queryByText("change-1")).toBeNull();
    expect(screen.queryByText("run-1")).toBeNull();
    expect(changeApi.query).toHaveBeenCalledWith("default", expect.objectContaining({ limit: 50 }));
    fireEvent.click(screen.getAllByRole("button", { name: "Open change" })[0]);
    expect(document.querySelector("tbody tr[aria-selected='true']")).toBeInTheDocument();
    expect(await screen.findByRole("dialog", { name: "Expenses · 100 entries" })).toBeInTheDocument();
    expect(await screen.findByText("Affected entries")).toBeInTheDocument();
    await waitFor(() => expect(changeApi.affectedEntry).toHaveBeenCalledWith("default", "change-1", "entry-1"));
    expect(setSearchParams).toHaveBeenCalledWith({ change: "change-1" });
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("searchbox", { name: "Search history" })).toHaveValue("");
  });

  it("Space History: announces list loading separately and renders the empty state", async () => {
    type ChangePage = Awaited<ReturnType<typeof changeApi.query>>;
    let finishQuery: ((page: ChangePage) => void) | undefined;
    vi.mocked(changeApi.query).mockImplementation(() =>
      new Promise<ChangePage>((resolve) => { finishQuery = resolve; })
    );
    render(() => <SpaceHistoryRoute />);

    expect(document.querySelector(".space-history")).toHaveAttribute("aria-busy", "true");
    expect(screen.getByRole("status")).toHaveTextContent("Loading space history...");
    expect(screen.queryByRole("table")).toBeNull();

    finishQuery?.({ changes: [], next_cursor: null });
    expect(await screen.findByText("No changes yet. Create, edit, delete, or restore an entry and it will appear here.")).toBeInTheDocument();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("Space History: retries a failed timeline request", async () => {
    type ChangePage = Awaited<ReturnType<typeof changeApi.query>>;
    let rejectQuery: ((reason?: unknown) => void) | undefined;
    vi.mocked(changeApi.query).mockImplementationOnce(() =>
      new Promise<ChangePage>((_resolve, reject) => { rejectQuery = reject; })
    );
    render(() => <SpaceHistoryRoute />);
    await waitFor(() => expect(changeApi.query).toHaveBeenCalledTimes(1));
    rejectQuery?.(new Error("offline"));

    expect(await screen.findByRole("alert")).toHaveTextContent("Failed to load space history.");
    (screen.getByRole("button", { name: "Retry" }) as HTMLButtonElement).click();
    expect(await screen.findByText("Expenses · 100 entries")).toBeInTheDocument();
    expect(changeApi.query).toHaveBeenCalledTimes(2);
  });

  it("Space History: keeps Change detail open and reports inspection failure", async () => {
    vi.mocked(changeApi.inspect).mockRejectedValue(new Error("offline"));
    render(() => <SpaceHistoryRoute />);
    fireEvent.click((await screen.findAllByRole("button", { name: "Open change" }))[0]);

    const dialog = await screen.findByRole("dialog", { name: "Expenses · 100 entries" });
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("Failed to load space history.");
    expect(dialog).toBeInTheDocument();
  });

  it("REQ-UX-DISMISS-001: closes and returns focus", async () => {
    render(() => <SpaceHistoryRoute />);
    const [opener] = await screen.findAllByRole("button", {
      name: "Open change",
    });
    fireEvent.click(opener);
    const dialog = await screen.findByRole("dialog", {
      name: "Expenses · 100 entries",
    });
    const close = within(dialog).getByRole("button", { name: "Close" });

    expect(close).toHaveAttribute("title", "Close");
    expect(close).toHaveClass("pill", "iconpill", "icononly");
    expect(close.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
    expect(close.querySelector(".ui-sr-only")).toHaveTextContent("Close");

    fireEvent.click(close);
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(opener));
  });

  it("REQ-UX-DISMISS-001: localizes the close name", async () => {
    setLocale("ja");
    render(() => <SpaceHistoryRoute />);
    const [opener] = await screen.findAllByRole("button", {
      name: "変更を開く",
    });
    fireEvent.click(opener);
    const dialog = await screen.findByRole("dialog");
    const close = within(dialog).getByRole("button", { name: "閉じる" });
    expect(close).toHaveAttribute("title", "閉じる");
  });

  it("localizes the technical detail labels", async () => {
    setLocale("ja");
    searchParams.value = { change: "change-1" };
    render(() => <SpaceHistoryRoute />);
    const dialog = await screen.findByRole("dialog");
    const technicalInfo = within(dialog).getByText("技術情報");
    fireEvent.click(technicalInfo);

    expect(await within(dialog).findByText("変更ID")).toBeVisible();
    expect(within(dialog).getByText("実行者ID")).toBeVisible();
    expect(within(dialog).getByText("Run ID")).toBeVisible();
    expect(within(dialog).getByText("エントリーID")).toBeVisible();
  });

  it("keeps the existing Change revert flow available from the Change detail", async () => {
    render(() => <SpaceHistoryRoute />);
    fireEvent.click((await screen.findAllByRole("button", { name: "Open change" }))[0]);
    fireEvent.click(await screen.findByRole("button", { name: "Revert this change" }));
    expect(await screen.findByText("Append a revert for this Change?")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Confirm revert" }));
    expect(await screen.findByText("Revert added to history.")).toBeInTheDocument();
    expect(screen.queryByText("inverse-change")).toBeNull();
    expect(changeApi.revert).toHaveBeenCalledWith("default", "change-1", {});
    expect(setSearchParams).toHaveBeenCalledWith(
      { change: undefined },
      { state: { historyRecoveryNotice: "Revert added to history." } },
    );
    expect(changeApi.query).toHaveBeenCalledTimes(3);

    cleanup();
    render(() => <SpaceHistoryRoute />);
    expect(await screen.findByText("Revert added to history.")).toBeInTheDocument();
  });

  it("Space History: reports a rejected recovery without claiming success", async () => {
    vi.mocked(changeApi.revert).mockRejectedValue({
      status: 403,
      mutationOutcome: "rejected",
    });
    render(() => <SpaceHistoryRoute />);
    fireEvent.click((await screen.findAllByRole("button", { name: "Open change" }))[0]);
    fireEvent.click(await screen.findByRole("button", { name: "Revert this change" }));
    fireEvent.click(await screen.findByRole("button", { name: "Confirm revert" }));

    expect(await screen.findByText("The recovery request was rejected. Review the error before trying again.")).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("button", { name: "Retry history refresh" })).toBeNull();
  });

  it("Space History: reports a revision conflict and refreshes the timeline", async () => {
    vi.mocked(changeApi.revert).mockRejectedValue({
      code: "REVISION_CONFLICT",
      status: 409,
    });
    render(() => <SpaceHistoryRoute />);
    fireEvent.click((await screen.findAllByRole("button", { name: "Open change" }))[0]);
    fireEvent.click(await screen.findByRole("button", { name: "Revert this change" }));
    fireEvent.click(await screen.findByRole("button", { name: "Confirm revert" }));

    expect(await screen.findByText("This change conflicts with newer history. Refresh the timeline and review it before trying again.")).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(changeApi.query).toHaveBeenCalledWith("default", expect.objectContaining({ limit: 50 }));
  });

  it("REQ-UX-DISMISS-001: traps focus and closes on Escape", async () => {
    render(() => <SpaceHistoryRoute />);
    const opener = (await screen.findAllByRole("button", { name: "Open change" }))[0];
    fireEvent.click(opener);
    const dialog = await screen.findByRole("dialog", { name: "Expenses · 100 entries" });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Close" }));
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Undo run" }));
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(opener));
  });

  it("REQ-UX-DISMISS-001: closes on backdrop and returns focus", async () => {
    render(() => <SpaceHistoryRoute />);
    const [opener] = await screen.findAllByRole("button", {
      name: "Open change",
    });
    fireEvent.click(opener);
    const dialog = await screen.findByRole("dialog", {
      name: "Expenses · 100 entries",
    });
    const backdrop = dialog.parentElement;
    if (!backdrop) throw new Error("history detail backdrop not found");

    fireEvent.click(backdrop);
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(opener));
  });

  it("blocks another recovery attempt until an unknown result is reconciled", async () => {
    let calls = 0;
    vi.mocked(changeApi.query).mockImplementation(async () => {
      calls += 1;
      if (calls === 3) throw new Error("offline");
      return { changes: [row("change-1", 100, "run-1")], next_cursor: null };
    });
    vi.mocked(changeApi.revert).mockRejectedValue(new Error("connection lost"));
    render(() => <SpaceHistoryRoute />);
    fireEvent.click((await screen.findAllByRole("button", { name: "Open change" }))[0]);
    fireEvent.click(await screen.findByRole("button", { name: "Revert this change" }));
    fireEvent.click(await screen.findByRole("button", { name: "Confirm revert" }));
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
      if (request.limit === 50 && !request.actor_principal_id && !request.run_id) {
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
    await waitFor(() => expect(document.querySelector(".space-history")).not.toHaveAttribute("aria-busy", "true"));
    fireEvent.click((await screen.findAllByRole("button", { name: "Open change" }))[0]);
    await screen.findByRole("dialog", { name: "Expenses · 1 entries" });
    fireEvent.click(await screen.findByRole("button", { name: "Revert this change" }));
    fireEvent.click(await screen.findByRole("button", { name: "Confirm revert" }));
    expect(await screen.findByText("The recovery is confirmed in history.")).toBeInTheDocument();
    expect(requests.some((request) => request.limit === 50 && !request.run_id && !request.actor_principal_id)).toBe(true);
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

  it("counts target Forms without comparable field groups", async () => {
    vi.mocked(changeApi.query).mockResolvedValue({
      changes: [{
        ...row("multi-form-change", 2),
        summary: {
          affected_entry_count: 2,
          target_form_ids: ["form-1", "form-2"],
          field_groups: row("multi-form-change", 2).summary.field_groups,
        },
      }],
      next_cursor: null,
    });

    render(() => <SpaceHistoryRoute />);

    expect(await screen.findByText("2 Forms · 2 entries")).toBeInTheDocument();
    expect(screen.queryByText("Expenses · 2 entries")).toBeNull();
  });

  it("does not infer the complete target Forms from field groups alone", async () => {
    vi.mocked(changeApi.query).mockResolvedValue({
      changes: [{
        ...row("legacy-summary", 2),
        summary: {
          affected_entry_count: 2,
          target_form_ids: [],
          field_groups: row("legacy-summary", 2).summary.field_groups,
        },
      }],
      next_cursor: null,
    });

    render(() => <SpaceHistoryRoute />);

    expect(await screen.findByText(/2 entries/)).toBeInTheDocument();
    expect(screen.queryByText("Expenses · 2 entries")).toBeNull();
    expect(screen.queryByText("1 Forms · 2 entries")).toBeNull();
  });

  it("opens a direct detail URL with related Run Changes and collapsed identifiers", async () => {
    searchParams.value = { change: "change-1" };
    render(() => <SpaceHistoryRoute />);
    const dialog = await screen.findByRole("dialog", { name: "Expenses · 100 entries" });
    await waitFor(() => expect(changeApi.query).toHaveBeenCalledWith("default", { limit: 10, run_id: "run-1" }));
    expect(screen.getByText("Related changes in this Run")).toBeInTheDocument();
    const technicalInfo = document.querySelector<HTMLDetailsElement>(".history-technical-info");
    expect(technicalInfo?.open).toBe(false);
    expect(within(dialog).getByText("change-1")).not.toBeVisible();
    expect(within(dialog).getByText("human:editor")).not.toBeVisible();
    expect(within(dialog).getByText("run-1")).not.toBeVisible();
    expect(within(dialog).getByText("entry-1")).not.toBeVisible();
    fireEvent.click(within(dialog).getByText("Technical info"));
    expect(await within(dialog).findByText("Change ID")).toBeVisible();
    expect(within(dialog).getByText("Actor ID")).toBeVisible();
    expect(within(dialog).getByText("Run ID")).toBeVisible();
    expect(await within(dialog).findByText("entry-1")).toBeVisible();
    expect(within(dialog).getByText("human:editor")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(setSearchParams).toHaveBeenCalledWith({ change: undefined });
  });

  it("Space History: keeps actor IDs out of labels when display names echo them", async () => {
    const actorId = "123e4567-e89b-12d3-a456-426614174000";
    const source = row("change-1", 1, "run-1");
    const unresolvedRow = {
      ...source,
      change: { ...source.change, actor_principal_id: actorId },
    };
    vi.mocked(spaceApi.listMembers).mockResolvedValue([{
      principal: {
        principal_id: actorId,
        display_name: actorId,
        kind: "human",
        state: "active",
      },
      role: "owner",
    }]);
    vi.mocked(changeApi.query).mockResolvedValue({
      changes: [unresolvedRow],
      next_cursor: null,
    });
    vi.mocked(changeApi.inspect).mockResolvedValue({
      change_id: "change-1",
      change: unresolvedRow.change,
      target_visibility: "complete",
      summary: source.summary,
      targets: [{
        form_id: "form-1",
        entry_id: "entry-1",
        before_revision_id: "rev-0",
        after_revision_id: "rev-1",
        operation: "update",
        fields: [{
          field_id: 1,
          before: { state: "value", value: "Travel" },
          after: { state: "value", value: "Business travel" },
        }],
      }],
      next_cursor: null,
    });

    render(() => <SpaceHistoryRoute />);
    const historyRow = await screen.findByRole("row", { name: /Unknown actor/ });
    expect(historyRow).not.toHaveTextContent(actorId);

    fireEvent.click(screen.getByText("Columns, filters, and sort"));
    const actorFilter = screen.getByLabelText("Filter by actor");
    expect(within(actorFilter).getAllByRole("option")).toHaveLength(1);
    expect(within(actorFilter).queryByRole("option", { name: actorId })).toBeNull();

    fireEvent.click(within(historyRow).getByRole("button", { name: "Open change" }));
    const dialog = await screen.findByRole("dialog");
    const technicalActorId = within(dialog).getByText(actorId);
    expect(technicalActorId).not.toBeVisible();
    fireEvent.click(within(dialog).getByText("Technical info"));
    expect(technicalActorId).toBeVisible();
  });

  it("REQ-UX-PAGINATION-001: uses the shared pager for affected detail pages", async () => {
    type Inspection = Awaited<ReturnType<typeof changeApi.inspect>>;
    let resolveSecondPage: ((value: Inspection) => void) | undefined;
    vi.mocked(changeApi.inspect).mockImplementation(async (_spaceId, changeId, options = {}) => {
      const source = row(changeId, 2);
      const secondPage = options.cursor === "entry-page-2";
      const page: Inspection = {
        change_id: changeId,
        change: source.change,
        target_visibility: "complete",
        summary: source.summary,
        targets: [{ form_id: "form-1", entry_id: secondPage ? "entry-2" : "entry-1", before_revision_id: null, after_revision_id: secondPage ? "revision-2" : "revision-1", operation: "create", fields: [] }],
        next_cursor: secondPage ? null : "entry-page-2",
      };
      if (secondPage) {
        return await new Promise<Inspection>((resolve) => {
          resolveSecondPage = resolve;
        });
      }
      return page;
    });
    render(() => <SpaceHistoryRoute />);
    fireEvent.click((await screen.findAllByRole("button", { name: "Open change" }))[0]);
    await screen.findByRole("dialog", { name: "Expenses · 100 entries" });
    expect(await screen.findByText(/Entry 1/)).toBeInTheDocument();
    const pagination = document.querySelector(".history-affected-layout .history-detail-pagination")!;
    expect(pagination).toHaveAttribute("aria-label", "Affected entries");
    const next = within(pagination).getByRole("button", { name: "Next" });
    fireEvent.click(next);
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Loading space history...",
    );
    expect(next).toBeDisabled();
    const source = row("change-1", 2);
    resolveSecondPage?.({
      change_id: "change-1",
      change: source.change,
      target_visibility: "complete",
      summary: source.summary,
      targets: [{ form_id: "form-1", entry_id: "entry-2", before_revision_id: null, after_revision_id: "revision-2", operation: "create", fields: [] }],
      next_cursor: null,
    });
    await waitFor(() => expect(changeApi.affectedEntry).toHaveBeenCalledWith("default", "change-1", "entry-2"));
    expect(document.querySelectorAll(".history-affected-list li")).toHaveLength(1);
    await waitFor(() => expect(screen.getByRole("button", { name: "Previous" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Previous" }));
    await waitFor(() => expect(changeApi.inspect).toHaveBeenLastCalledWith("default", "change-1", { limit: 10, cursor: undefined }));
    expect(document.querySelectorAll(".history-affected-list li")).toHaveLength(1);
  });

  it("REQ-UX-PAGINATION-001: uses the shared pager for related history pages", async () => {
    vi.mocked(changeApi.query).mockImplementation(async (_spaceId, request) => {
      if (!request.run_id) {
        return { changes: [row("change-1", 100, "run-1")], next_cursor: null };
      }
      const secondPage = request.cursor === "related-page-2";
      return {
        changes: [row(secondPage ? "change-3" : "change-2", 30, "run-1")],
        next_cursor: secondPage ? null : "related-page-2",
      };
    });
    render(() => <SpaceHistoryRoute />);
    fireEvent.click((await screen.findAllByRole("button", { name: "Open change" }))[0]);
    await screen.findByRole("dialog", { name: "Expenses · 100 entries" });

    const pagination = await screen.findByRole("navigation", {
      name: "Related changes in this Run",
    });
    const next = within(pagination).getByRole("button", { name: "Next" });
    expect(next).toHaveAttribute("title", "Next");
    fireEvent.click(next);
    await waitFor(() => expect(changeApi.query).toHaveBeenLastCalledWith(
      "default",
      { limit: 10, run_id: "run-1", cursor: "related-page-2" },
    ));
    await waitFor(() => expect(screen.getByRole("button", { name: "Previous" })).toBeEnabled());
    const currentPagination = screen.getByRole("navigation", {
      name: "Related changes in this Run",
    });
    expect(within(currentPagination).getByRole("button", { name: "Previous" }))
      .toBeInTheDocument();
    expect(within(currentPagination).getByRole("button", { name: "Next" }))
      .toBeDisabled();
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
