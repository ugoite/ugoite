import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { formatDateTimeLabel } from "~/lib/date-format";
import { changeApi } from "~/lib/ugoite-client";
import SpaceHistoryRoute from "./history";

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>{props.children}</a>
  ),
  useParams: () => ({ space_id: "default" }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  changeApi: { list: vi.fn(), revert: vi.fn(), undoRun: vi.fn() },
}));

describe("space history route", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    setLocale("en");
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

    expect(await screen.findByText("Space history")).toBeInTheDocument();
    // Revert rows are labeled; the reverted Change is kept, not rewritten.
    expect(await screen.findByText("Revert")).toBeInTheDocument();
    expect(await screen.findByText("Change")).toBeInTheDocument();
    expect(await screen.findByText("Restore entry")).toBeInTheDocument();
    expect(await screen.findByText("human:owner")).toBeInTheDocument();
    expect(await screen.findByText("human:editor")).toBeInTheDocument();
    expect(
      await screen.findByText(
        formatDateTimeLabel(createdAtMicros / 1000),
      ),
    ).toBeInTheDocument();
    // Advanced detail, shown as-is.
    expect(await screen.findByText("Change change-2")).toBeInTheDocument();
    expect(await screen.findByText("Run run-7")).toBeInTheDocument();
    expect(changeApi.list).toHaveBeenCalledWith("default");
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
