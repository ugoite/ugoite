import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { formatDateTimeLabel } from "~/lib/date-format";
import { changeApi, spaceApi } from "~/lib/ugoite-client";
import SpaceHistoryRoute from "./history";

vi.mock("@solidjs/router", () => ({
  A: (props: {
    href: string;
    class?: string;
    children: unknown;
    "aria-label"?: string;
    title?: string;
  }) => (
    <a
      href={props.href}
      class={props.class}
      aria-label={props["aria-label"]}
      title={props.title}
    >
      {props.children}
    </a>
  ),
  useParams: () => ({ space_id: "default" }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  changeApi: { list: vi.fn(), revert: vi.fn(), undoRun: vi.fn() },
  spaceApi: { listMembers: vi.fn() },
}));

describe("space history route", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    setLocale("en");
    vi.mocked(spaceApi.listMembers).mockResolvedValue([]);
  });

  it("renders the append-only timeline without inventing targets", async () => {
    const createdAtMicros = 1767225600000000;
    vi.mocked(changeApi.list).mockResolvedValue([
      {
        change_id: "change-2",
        generation: 2,
        actor_principal_id: "human:owner",
        message: "Restore entry",
        reverts_change_id: "change-1",
        run_id: null,
        created_at_micros: createdAtMicros,
      },
      {
        change_id: "change-1",
        generation: 1,
        actor_principal_id: "human:editor",
        message: null,
        reverts_change_id: null,
        run_id: "run-7",
        created_at_micros: createdAtMicros - 1000000,
      },
    ]);

    render(() => <SpaceHistoryRoute />);

    // Revert rows are labeled; the reverted Change is kept, not rewritten.
    expect(await screen.findByRole("columnheader", { name: "Change" }))
      .toBeInTheDocument();
    expect(await screen.findByRole("columnheader", { name: "Actor" }))
      .toBeInTheDocument();
    expect((await screen.findAllByText("Revert")).length).toBeGreaterThan(0);
    expect(await screen.findByText("Restore entry")).toBeInTheDocument();
    expect(await screen.findByText("human:owner")).toBeInTheDocument();
    expect(await screen.findByText("human:editor")).toBeInTheDocument();
    expect(
      await screen.findByText(
        formatDateTimeLabel(createdAtMicros / 1000),
      ),
    ).toBeInTheDocument();
    // Exact IDs stay advanced-only inside the row disclosure.
    expect(screen.getByText("change-2").closest("details")).not.toBeNull();
    const disclosures = await screen.findAllByText("View details");
    expect(disclosures).toHaveLength(2);
    expect(document.querySelector(".historyTable")).toBeInTheDocument();
    expect(document.querySelector(".historyTable.ui-card")).toBeNull();
    expect(changeApi.list).toHaveBeenCalledWith("default");
  });

  it("PR6: resolves actor IDs to member display names without raw UUIDs in rows", async () => {
    vi.mocked(changeApi.list).mockResolvedValue([
      {
        change_id: "change-9",
        generation: 9,
        actor_principal_id: "01900000-0000-7000-8000-000000000042",
        message: null,
        reverts_change_id: null,
        run_id: null,
        created_at_micros: 1767225600000000,
      },
    ]);
    vi.mocked(spaceApi.listMembers).mockResolvedValue([
      {
        principal: {
          principal_id: "01900000-0000-7000-8000-000000000042",
          display_name: "Ada Example",
          kind: "human",
          state: "active",
        },
        role: "owner",
      },
    ]);

    render(() => <SpaceHistoryRoute />);

    expect(await screen.findByText("Ada Example")).toBeInTheDocument();
    expect(
      screen.queryByText("01900000-0000-7000-8000-000000000042"),
    ).toBeNull();
  });

  it("renders the empty state when no changes exist", async () => {
    vi.mocked(changeApi.list).mockResolvedValue([]);

    render(() => <SpaceHistoryRoute />);

    expect(await screen.findByText(/No changes yet/)).toBeInTheDocument();
  });

  it("reverts a change as a newly appended Change after confirmation", async () => {
    vi.mocked(changeApi.list)
      .mockResolvedValueOnce([
        {
          change_id: "change-1",
          generation: 1,
          actor_principal_id: "human:owner",
          message: null,
          reverts_change_id: null,
          run_id: null,
          created_at_micros: 1767225600000000,
        },
      ])
      .mockResolvedValue([]);
    vi.mocked(changeApi.revert).mockResolvedValue({
      change_id: "change-2",
      reverts_change_id: "change-1",
      run_id: null,
    });

    render(() => <SpaceHistoryRoute />);
    fireEvent.click(
      await screen.findByRole("button", { name: "Revert this change" }),
    );

    // The append-only notice is explicit before the operation.
    expect(await screen.findByText(/appends a new Change/))
      .toBeInTheDocument();
    fireEvent.click(
      await screen.findByRole("button", { name: "Append new Change" }),
    );

    expect(await screen.findByText("Reverted as Change change-2."))
      .toBeInTheDocument();
    expect(changeApi.revert).toHaveBeenCalledWith(
      "default",
      "change-1",
      {},
    );
    // The timeline refreshes from the server-confirmed result.
    expect(changeApi.list).toHaveBeenCalledTimes(2);
  });

  it("audit_baseline_revert_success_retains_confirmation_dialog", async () => {
    vi.mocked(changeApi.list)
      .mockResolvedValueOnce([
        {
          change_id: "audit-change-before",
          generation: 1,
          actor_principal_id: "human:owner",
          message: null,
          reverts_change_id: null,
          run_id: null,
          created_at_micros: 1767225600000000,
        },
      ])
      .mockResolvedValue([]);
    let resolveRevert!: (
      value: Awaited<ReturnType<typeof changeApi.revert>>,
    ) => void;
    vi.mocked(changeApi.revert).mockReturnValue(
      new Promise((resolve) => {
        resolveRevert = resolve;
      }),
    );

    render(() => <SpaceHistoryRoute />);
    fireEvent.click(
      await screen.findByRole("button", { name: "Revert this change" }),
    );
    const dialog = await screen.findByRole("dialog");
    const confirm = screen.getByRole("button", { name: "Append new Change" });
    fireEvent.click(confirm);
    fireEvent.click(confirm);
    expect(confirm).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(dialog).toBeInTheDocument();
    expect(changeApi.revert).toHaveBeenCalledTimes(1);

    resolveRevert({
      change_id: "audit-change-inverse",
      reverts_change_id: "audit-change-before",
      run_id: null,
    });
    expect(await screen.findByText("Reverted as Change audit-change-inverse."))
      .toBeInTheDocument();
    await waitFor(() => expect(confirm).toBeEnabled());
    expect(changeApi.revert).toHaveBeenCalledTimes(1);
    expect(changeApi.revert).toHaveBeenCalledWith(
      "default",
      "audit-change-before",
      {},
    );
    expect(changeApi.list).toHaveBeenCalledTimes(2);

    // F05 baseline defect, not the desired contract: the successful operation
    // must remove this dialog so its confirmation cannot be submitted again.
    // This mocked response does not prove an actual Change was committed.
    expect(screen.getByRole("dialog")).toBe(dialog);
  });

  it("offers Run undo only when the response carries a Run ID", async () => {
    vi.mocked(changeApi.list).mockResolvedValue([
      {
        change_id: "change-1",
        generation: 1,
        actor_principal_id: "human:owner",
        message: null,
        reverts_change_id: null,
        run_id: "run-7",
        created_at_micros: 1767225600000000,
      },
      {
        change_id: "change-0",
        generation: 0,
        actor_principal_id: "human:owner",
        message: null,
        reverts_change_id: null,
        run_id: null,
        created_at_micros: 1767225500000000,
      },
    ]);
    vi.mocked(changeApi.undoRun).mockResolvedValue({
      run_id: "run-7",
      reverted_change_count: 1,
    });

    render(() => <SpaceHistoryRoute />);
    const undoButtons = await screen.findAllByRole("button", {
      name: "Undo run",
    });
    expect(undoButtons).toHaveLength(1);
    fireEvent.click(undoButtons[0]);
    fireEvent.click(
      await screen.findByRole("button", { name: "Append new Change" }),
    );

    expect(await screen.findByText("Undid 1 change(s) for this run."))
      .toBeInTheDocument();
    expect(changeApi.undoRun).toHaveBeenCalledWith("default", "run-7");
  });

  it("leaves Knowledge unchanged when recovery fails", async () => {
    vi.mocked(changeApi.list).mockResolvedValue([
      {
        change_id: "change-1",
        generation: 1,
        actor_principal_id: "human:owner",
        message: null,
        reverts_change_id: null,
        run_id: null,
        created_at_micros: 1767225600000000,
      },
    ]);
    vi.mocked(changeApi.revert).mockRejectedValue(new Error("conflict"));

    render(() => <SpaceHistoryRoute />);
    fireEvent.click(
      await screen.findByRole("button", { name: "Revert this change" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Append new Change" }),
    );

    expect(await screen.findByText(/Knowledge is unchanged/))
      .toBeInTheDocument();
    // No refresh: the failed operation appended nothing.
    expect(changeApi.list).toHaveBeenCalledTimes(1);
  });

  it("renders a recoverable error state", async () => {
    vi.mocked(changeApi.list).mockRejectedValue(new Error("forbidden"));

    render(() => <SpaceHistoryRoute />);

    expect(await screen.findByText(/Failed to load space history/))
      .toBeInTheDocument();
  });
});
