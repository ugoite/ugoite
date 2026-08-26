import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
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
    vi.stubGlobal("confirm", vi.fn(() => true));
    vi.mocked(spaceApi.list).mockReset();
  });

  it("keeps CLI device authorization outside the v0.1 UI", async () => {
    fetchMock.mockResolvedValue({
      ok: true,
      json: async () => ({ resource: null }),
    });

    render(() => <DeviceApprovalRoute />);

    expect(await screen.findByText("CLI device authorization is not available"))
      .toBeInTheDocument();
    expect(spaceApi.list).not.toHaveBeenCalled();
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
        }),
      })
      .mockResolvedValueOnce({ ok: true, json: async () => ({}) });
    vi.mocked(spaceApi.list).mockResolvedValue([{
      id: "space-1",
      name: "Docs",
    }]);

    render(() => <DeviceApprovalRoute />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Approve MCP access" }),
    );
    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        "/api/oauth/device/approve",
        expect.objectContaining({ method: "POST" }),
      )
    );
    expect(
      await screen.findByText("MCP access approved. Return to the MCP client."),
    )
      .toBeInTheDocument();
  });
});
