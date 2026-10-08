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
import type { NodeAuditEvent } from "~/lib/types";

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
  });
});
