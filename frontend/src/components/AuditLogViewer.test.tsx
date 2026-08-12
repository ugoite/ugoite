import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  AuditLogViewer,
  NodeAuditLogViewer,
  SpaceAuditLogViewer,
} from "./AuditLogViewer";
import { setLocale } from "~/lib/i18n";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import type { NodeAuditEvent, SpaceAuditEvent } from "~/lib/types";
import { authApi, spaceApi } from "~/lib/ugoite-client";

vi.mock("~/lib/ugoite-client", () => ({
  authApi: { listAudit: vi.fn() },
  spaceApi: { listAudit: vi.fn() },
}));

const nodeEvent = (
  index: number,
  overrides: Partial<NodeAuditEvent> = {},
): NodeAuditEvent => ({
  event_id: `node-event-${index}`,
  timestamp: `2026-08-12T00:${String(index).padStart(2, "0")}:00Z`,
  node_id: "node-1",
  subject_account_id: "subject-1",
  actor_account_id: index % 2 === 0 ? "actor-1" : "actor-2",
  credential_id: null,
  action: index % 2 === 0 ? "session.revoked" : "authorization.denied",
  target_type: "browser_session",
  target_id: `target-${index}`,
  outcome: index % 2 === 0 ? "success" : "deny",
  request_id: null,
  safe_metadata: {},
  ...overrides,
});

const spaceEvent = (
  index: number,
  overrides: Partial<SpaceAuditEvent> = {},
): SpaceAuditEvent => ({
  event_id: `space-event-${index}`,
  timestamp: `2026-08-12T00:${String(index).padStart(2, "0")}:00Z`,
  space_id: "space-1",
  action: "authorization.denied",
  subject_principal_id: "principal-1",
  actor_principal_id: "actor-1",
  credential_id: null,
  outcome: "deny",
  target_type: "authorization",
  target_id: `target-${index}`,
  request_method: "GET",
  request_path: "/spaces/space-1/entries",
  request_id: null,
  metadata: { required_action: "read" },
  prev_hash: `prev-${index}`,
  event_hash: `hash-${index}`,
  ...overrides,
});

describe("AuditLogViewer", () => {
  beforeEach(() => {
    setLocale("en");
    vi.mocked(authApi.listAudit).mockReset();
    vi.mocked(spaceApi.listAudit).mockReset();
  });

  it("filters node events locally and resets to the first page", async () => {
    vi.mocked(authApi.listAudit).mockResolvedValue([
      nodeEvent(0),
      nodeEvent(1),
    ]);
    render(() => <NodeAuditLogViewer />);

    expect(await screen.findByText("session.revoked")).toBeInTheDocument();
    fireEvent.input(screen.getByPlaceholderText("Exact action"), {
      target: { value: "authorization.denied" },
    });

    await waitFor(() => {
      expect(screen.getByText("authorization.denied")).toBeInTheDocument();
      expect(screen.queryByText("session.revoked")).toBeNull();
      expect(authApi.listAudit).toHaveBeenCalledTimes(2);
    });
  });

  it("pages the bounded node response", async () => {
    vi.mocked(authApi.listAudit).mockResolvedValue(
      Array.from({ length: 30 }, (_, index) => nodeEvent(index)),
    );
    render(() => <NodeAuditLogViewer />);

    expect(await screen.findAllByText("authorization.denied")).not.toHaveLength(
      0,
    );
    const next = screen.getByRole("button", { name: "Next" });
    expect(next).not.toBeDisabled();
    fireEvent.click(next);

    await waitFor(() => {
      expect(screen.getByText("Page 2 of 2 · 30 events")).toBeInTheDocument();
      expect(screen.getAllByText("session.revoked")).not.toHaveLength(0);
    });
  });

  it("sends Space filters and server offsets to the existing endpoint", async () => {
    vi.mocked(spaceApi.listAudit).mockResolvedValue({
      items: [spaceEvent(0)],
      total: 50,
      offset: 0,
      limit: 25,
    });
    render(() => <SpaceAuditLogViewer spaceId="space-1" />);

    await screen.findByText("authorization.denied");
    fireEvent.input(screen.getByPlaceholderText("Exact action"), {
      target: { value: "authorization.denied" },
    });
    fireEvent.input(screen.getByPlaceholderText("Exact actor ID"), {
      target: { value: "actor-1" },
    });
    fireEvent.change(screen.getByLabelText("Outcome"), {
      target: { value: "deny" },
    });

    await waitFor(() => {
      expect(spaceApi.listAudit).toHaveBeenLastCalledWith("space-1", {
        offset: 0,
        limit: 25,
        action: "authorization.denied",
        actorId: "actor-1",
        outcome: "deny",
      });
    });

    fireEvent.click(await screen.findByRole("button", { name: "Next" }));
    await waitFor(() => {
      expect(spaceApi.listAudit).toHaveBeenLastCalledWith("space-1", {
        offset: 25,
        limit: 25,
        action: "authorization.denied",
        actorId: "actor-1",
        outcome: "deny",
      });
    });
  });

  it("shows request details and tolerates system events without an actor", async () => {
    vi.mocked(spaceApi.listAudit).mockResolvedValue({
      items: [spaceEvent(0, { actor_principal_id: null })],
      total: 1,
      offset: 0,
      limit: 25,
    });
    render(() => <SpaceAuditLogViewer spaceId="space-1" />);

    await screen.findByText("authorization.denied");
    fireEvent.click(screen.getByText("View details"));

    expect(screen.getByText("GET")).toBeInTheDocument();
    expect(screen.getByText("/spaces/space-1/entries")).toBeInTheDocument();
  });

  it("shows empty and localized API failure states", async () => {
    vi.mocked(authApi.listAudit).mockResolvedValue([]);
    render(() => <NodeAuditLogViewer />);
    expect(await screen.findByText("No audit events yet.")).toBeInTheDocument();

    vi.mocked(authApi.listAudit).mockRejectedValueOnce(
      new UgoiteApiError({
        kind: "forbidden",
        code: "FORBIDDEN",
        status: 403,
        message: "forbidden",
        detail: { request_id: "audit-1" },
      }),
    );
    render(() => <NodeAuditLogViewer />);
    const alert = await screen.findAllByRole("alert");
    expect(alert.at(-1)).toHaveTextContent("You do not have permission");
    expect(alert.at(-1)).toHaveTextContent("audit-1");
  });

  it("localizes known outcome labels", async () => {
    setLocale("ja");
    vi.mocked(authApi.listAudit).mockResolvedValue([nodeEvent(0)]);
    render(() => <NodeAuditLogViewer />);

    expect(await screen.findByText("成功")).toBeInTheDocument();
  });

  it("renders the shared viewer with a custom loader", async () => {
    const load = vi.fn().mockResolvedValue({
      items: [nodeEvent(0)],
      total: 1,
      offset: 0,
      limit: 25,
    });
    render(() => <AuditLogViewer source="space" load={load} />);
    expect(await screen.findByText("session.revoked")).toBeInTheDocument();
    expect(load).toHaveBeenCalledWith({
      offset: 0,
      limit: 25,
      filters: { action: "", actorId: "", outcome: "" },
    });
  });
});
