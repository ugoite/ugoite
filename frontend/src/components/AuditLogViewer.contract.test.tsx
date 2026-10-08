import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AuditLogViewer } from "./AuditLogViewer";
import { setLocale } from "~/lib/i18n";
import type { NodeAuditEvent, SpaceAuditEvent } from "~/lib/types";

vi.mock("~/lib/ugoite-client", () => ({
  authApi: { listAudit: vi.fn() },
  spaceApi: { listAudit: vi.fn(), listMembers: vi.fn() },
}));

const event: NodeAuditEvent = {
  event_id: "audit-event-1",
  timestamp: "2026-10-08T10:00:00Z",
  node_id: "node-1",
  subject_account_id: "subject-1",
  actor_account_id: "actor-1",
  credential_id: "credential-1",
  action: "authorization.denied",
  target_type: "browser_session",
  target_id: "target-1",
  outcome: "deny",
  request_id: "request-1",
  safe_metadata: { source: "router" },
};

const spaceEvent: SpaceAuditEvent = {
  event_id: "space-event-1",
  timestamp: "2026-10-08T10:05:00Z",
  space_id: "space-1",
  action: "authorization.denied",
  subject_principal_id: "space-subject-1",
  actor_principal_id: "space-actor-1",
  credential_id: "space-credential-1",
  outcome: "deny",
  target_type: "entry",
  target_id: "space-target-1",
  request_method: "POST",
  request_path: "/spaces/space-1/entries",
  request_id: "space-request-1",
  metadata: { source: "space-router" },
  prev_hash: "previous-hash-1",
  event_hash: "event-hash-1",
};

const page = (items: NodeAuditEvent[]) => ({
  items,
  total: items.length,
  offset: 0,
  limit: 25,
});

describe("AuditLogViewer component contract", () => {
  beforeEach(() => setLocale("en"));
  afterEach(cleanup);

  it("keeps the busy state until the page load finishes", async () => {
    let finishLoad!: (result: ReturnType<typeof page>) => void;
    const pendingPage = new Promise<ReturnType<typeof page>>((resolve) => {
      finishLoad = resolve;
    });
    const load = vi.fn(() => pendingPage);
    render(() => <AuditLogViewer source="node" load={load} />);

    const loadingStatus = screen.getByRole("status");
    expect(loadingStatus).toHaveTextContent("Loading audit events…");
    finishLoad(page([]));

    expect(await screen.findByText("No audit events yet.")).toBeInTheDocument();
  });

  it("shows the no-matches state after a filtered page has no rows", async () => {
    const load = vi.fn()
      .mockResolvedValueOnce(page([event]))
      .mockResolvedValueOnce(page([]));
    render(() => <AuditLogViewer source="node" load={load} />);

    expect(await screen.findByText("authorization.denied")).toBeInTheDocument();
    fireEvent.input(screen.getByPlaceholderText("Exact action"), {
      target: { value: "missing.action" },
    });

    expect(
      await screen.findByText("No audit events match these filters."),
    ).toBeInTheDocument();
    await waitFor(() => expect(load).toHaveBeenCalledTimes(2));
  });

  it("replaces stale rows with a localized error after a failed query", async () => {
    const load = vi.fn()
      .mockResolvedValueOnce(page([event]))
      .mockRejectedValueOnce(new Error("unavailable"));
    render(() => <AuditLogViewer source="node" load={load} />);

    expect(await screen.findByText("authorization.denied")).toBeInTheDocument();
    fireEvent.input(screen.getByPlaceholderText("Exact action"), {
      target: { value: "next.action" },
    });

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Failed to load node audit events.",
    );
    expect(screen.queryByText("authorization.denied")).not.toBeInTheDocument();
  });

  it("updates the page count through previous and next controls", async () => {
    const load = vi.fn().mockResolvedValue({
      ...page(
        Array.from({ length: 30 }, (_, index) => ({
          ...event,
          event_id: `audit-event-${index}`,
        })),
      ),
      total: 30,
    });
    render(() => <AuditLogViewer source="node" load={load} />);

    expect(await screen.findByText("Page 1 of 2 · 30 events"))
      .toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText("Page 2 of 2 · 30 events"))
      .toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Previous" }));
    expect(await screen.findByText("Page 1 of 2 · 30 events"))
      .toBeInTheDocument();
  });

  it("resets paging before applying a filter", async () => {
    const events = Array.from({ length: 30 }, (_, index) => ({
      ...event,
      event_id: `filter-reset-${index}`,
      action: index < 25 ? "authorization.denied" : "session.revoked",
    }));
    const load = vi.fn(({
      offset,
      limit,
      filters,
    }: {
      offset: number;
      limit: number;
      filters: { action: string; actorId: string; outcome: string };
    }) => {
      const filtered = events.filter((item) =>
        !filters.action || item.action === filters.action
      );
      return Promise.resolve({
        items: filtered.slice(offset, offset + limit),
        total: filtered.length,
        offset,
        limit,
      });
    });
    render(() => <AuditLogViewer source="node" load={load} />);

    expect(await screen.findByText("Page 1 of 2 · 30 events"))
      .toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText("Page 2 of 2 · 30 events"))
      .toBeInTheDocument();
    fireEvent.input(screen.getByPlaceholderText("Exact action"), {
      target: { value: "authorization.denied" },
    });

    expect(await screen.findByText("Page 1 of 1 · 25 events"))
      .toBeInTheDocument();
    expect(load).toHaveBeenLastCalledWith({
      offset: 0,
      limit: 25,
      filters: { action: "authorization.denied", actorId: "", outcome: "" },
    });
  });

  it("discloses exact event identities only in the opened details", async () => {
    const load = vi.fn().mockResolvedValue(page([event]));
    render(() => (
      <AuditLogViewer
        source="node"
        load={load}
        actorDirectory={[{ principal_id: "actor-1", display_name: "Ada" }]}
      />
    ));

    const action = await screen.findByText("authorization.denied");
    const row = action.closest("tr");
    if (!row) throw new Error("Audit event row was not rendered");
    expect(row).toHaveTextContent("Ada");
    const details = row.querySelector("details");
    if (!details) throw new Error("Audit event details were not rendered");
    expect(details).not.toHaveAttribute("open");
    const primaryText = Array.from(row.cells)
      .filter((cell) => !cell.contains(details))
      .map((cell) => cell.textContent ?? "")
      .join(" ");
    for (
      const identifier of [
        "audit-event-1",
        "actor-1",
        "subject-1",
        "node-1",
        "credential-1",
        "request-1",
      ]
    ) {
      expect(primaryText).not.toContain(identifier);
      expect(within(details).getByText(identifier)).not.toBeVisible();
    }

    fireEvent.click(within(details).getByText("View details"));
    expect(details).toHaveAttribute("open");
    for (
      const identifier of [
        "audit-event-1",
        "actor-1",
        "subject-1",
        "node-1",
        "credential-1",
        "request-1",
      ]
    ) {
      expect(within(details).getByText(identifier)).toBeVisible();
    }
    expect(details).toHaveTextContent('"source": "router"');
  });

  it("discloses safe Space metadata and an optional event hash in details", async () => {
    const load = vi.fn().mockResolvedValue({
      items: [spaceEvent],
      total: 1,
      offset: 0,
      limit: 25,
    });
    render(() => <AuditLogViewer source="space" load={load} />);

    const action = await screen.findByText("authorization.denied");
    const row = action.closest("tr");
    if (!row) throw new Error("Audit event row was not rendered");
    const details = row.querySelector("details");
    if (!details) throw new Error("Audit event details were not rendered");
    const eventHash = within(details).getByText("event-hash-1");
    const metadata = within(details).getByText(/"source": "space-router"/);
    expect(eventHash).not.toBeVisible();
    expect(metadata).not.toBeVisible();

    fireEvent.click(within(details).getByText("View details"));
    expect(eventHash).toBeVisible();
    expect(metadata).toBeVisible();
  });
});
