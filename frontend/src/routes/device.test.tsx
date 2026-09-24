import "@testing-library/jest-dom/vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import DeviceApprovalRoute from "./device";
import { spaceApi } from "~/lib/ugoite-client";

const fetchMock = vi.fn();

vi.mock("@solidjs/router", () => ({
  useSearchParams: () => [{ user_code: "ABCD" }],
}));

vi.mock("~/lib/ugoite-client", () => ({
  spaceApi: {
    list: vi.fn(),
  },
}));

describe("/device", () => {
  beforeEach(() => {
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
    vi.mocked(spaceApi.list).mockReset();
  });

  it("explains unsupported resources with recovery guidance", async () => {
    fetchMock.mockResolvedValue({
      ok: true,
      json: async () => ({
        device_name: "Other client",
        requested_actions: ["read"],
        resource: "https://other.example/mcp",
      }),
    });

    render(() => <DeviceApprovalRoute />);

    expect(
      await screen.findByRole("heading", {
        name: "Unsupported device authorization",
      }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Approve/ })).toBeNull();
  });

  it("approves a supported REST CLI request", async () => {
    fetchMock.mockResolvedValue({
      ok: true,
      json: async () => ({
        device_name: "CLI",
        requested_actions: ["read", "create", "update"],
        resource: null,
      }),
    });
    vi.mocked(spaceApi.list).mockResolvedValue([{
      name: "Docs",
      space_uid: "space-uid-1",
    }]);

    render(() => <DeviceApprovalRoute />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Review CLI access request" }),
    );
    // Approval runs behind the shared confirmation dialog: nothing is sent
    // until the explicit confirm action.
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveAccessibleName("Approve CLI access?");
    expect(dialog).toHaveAccessibleDescription(
      "Approve CLI for actions: read, create, update?",
    );
    expect(fetchMock).toHaveBeenCalledTimes(1);
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Approve CLI access" }),
    );
    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        "/api/oauth/device/approve",
        expect.objectContaining({ method: "POST" }),
      )
    );
    expect(await screen.findByRole("heading", { name: "CLI access approved" }))
      .toBeInTheDocument();
  });

  it("cancelling the approval dialog sends nothing", async () => {
    fetchMock.mockResolvedValue({
      ok: true,
      json: async () => ({
        device_name: "CLI",
        requested_actions: ["read"],
        resource: null,
      }),
    });
    vi.mocked(spaceApi.list).mockResolvedValue([{
      name: "Docs",
      space_uid: "space-uid-1",
    }]);

    render(() => <DeviceApprovalRoute />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Review CLI access request" }),
    );
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Cancel" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(
      screen.queryByRole("heading", { name: "CLI access approved" }),
    ).toBeNull();
  });

  it("leaves the approval dialog retryable after a network failure", async () => {
    fetchMock
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          device_name: "CLI",
          requested_actions: ["read"],
          resource: null,
        }),
      })
      .mockRejectedValueOnce(new Error("network down"));
    vi.mocked(spaceApi.list).mockResolvedValue([{
      name: "Docs",
      space_uid: "space-uid-1",
    }]);

    render(() => <DeviceApprovalRoute />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Review CLI access request" }),
    );
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Approve CLI access" }),
    );
    await waitFor(() =>
      expect(
        within(dialog).getByRole("button", { name: "Approve CLI access" }),
      ).toBeEnabled()
    );
    expect(within(dialog).getByRole("alert")).toHaveTextContent(
      "The settings operation failed.",
    );
  });

  it("approves a supported MCP-scoped request", async () => {
    const resource = `${location.origin}/mcp`;
    fetchMock
      .mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          device_name: "MCP client",
          requested_actions: ["read"],
          resource,
          requested_space_uid: "space-uid-2",
        }),
      })
      .mockResolvedValueOnce({ ok: true, json: async () => ({}) });
    vi.mocked(spaceApi.list).mockResolvedValue([{
      name: "Docs",
      space_uid: "space-uid-1",
    }, {
      name: "Current Space",
      space_uid: "space-uid-2",
    }]);

    render(() => <DeviceApprovalRoute />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Review MCP access request" }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveAccessibleName("Approve MCP access?");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Approve MCP access" }),
    );
    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        "/api/oauth/device/approve",
        expect.objectContaining({ method: "POST" }),
      )
    );
    expect(JSON.parse(fetchMock.mock.calls[1][1].body)).toEqual({
      user_code: "ABCD",
      space_id: "space-uid-2",
      granted_actions: ["read"],
    });
    expect(await screen.findByRole("heading", { name: "MCP access approved" }))
      .toBeInTheDocument();
  });
});
