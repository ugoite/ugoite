import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { KonasePanel } from "./KonasePanel";
import type { BrowserMcpAuthorizationOptions } from "~/lib/konase/browser-mcp-auth";

const { getSpaceMock, authorizeMock } = vi.hoisted(() => ({
  getSpaceMock: vi.fn(),
  authorizeMock: vi.fn(),
}));

vi.mock("~/lib/ugoite-client", () => ({
  spaceApi: { get: getSpaceMock },
}));
vi.mock("~/lib/konase/browser-mcp-auth", () => ({
  authorizeBrowserMcp: authorizeMock,
}));

describe("KonasePanel Space authority", () => {
  beforeEach(() => {
    setLocale("en");
    getSpaceMock.mockReset();
    authorizeMock.mockReset();
  });

  it("starts MCP authorization from the rendered Space and drops the host when the Space changes", async () => {
    const [spaceId, setSpaceId] = createSignal("space-a");
    getSpaceMock.mockResolvedValue({
      id: "space-a",
      space_uid: "space-a-uid",
      name: "Space A",
      created_at: "",
    });
    authorizeMock.mockImplementation(
      async (options: BrowserMcpAuthorizationOptions) => {
        options.onApprovalRequired?.({
          verificationUriComplete: `${location.origin}/device?user_code=ABCD`,
          userCode: "ABCD",
        });
        return {
          accessToken: "space-a-token",
          endpoint: "/mcp",
          resource: `${location.origin}/mcp`,
          spaceUid: options.spaceUid,
        };
      },
    );

    render(() => (
      <>
        <KonasePanel spaceId={spaceId()} />
        <button type="button" onClick={() => setSpaceId("space-b")}>
          Switch Space
        </button>
      </>
    ));

    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));

    await waitFor(() =>
      expect(authorizeMock).toHaveBeenCalledWith(
        expect.objectContaining({
          spaceUid: "space-a-uid",
          deviceName: "Ugoite Browser Konase (space-a)",
        }),
      )
    );
    expect(getSpaceMock).toHaveBeenCalledWith("space-a");
    expect(screen.getByPlaceholderText(/Ask Konase/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Switch Space" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Connect Ugoite MCP" }))
        .toBeInTheDocument()
    );
    expect(screen.queryByPlaceholderText(/Ask Konase/)).not.toBeInTheDocument();
  });

  it("does not configure when the current Space has no server UID", async () => {
    getSpaceMock.mockResolvedValue({
      id: "space-a",
      name: "Space A",
      created_at: "",
    });
    render(() => <KonasePanel spaceId="space-a" />);

    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));

    await waitFor(() => expect(authorizeMock).not.toHaveBeenCalled());
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Konase could not complete the Work.",
    );
  });

  it("does not bind a credential if the Space changes during approval", async () => {
    const [spaceId, setSpaceId] = createSignal("space-a");
    let resolveAuthorization!: (credential: {
      accessToken: string;
      endpoint: string;
      resource: string;
      spaceUid: string;
    }) => void;
    getSpaceMock.mockResolvedValue({
      id: "space-a",
      space_uid: "space-a-uid",
      name: "Space A",
      created_at: "",
    });
    authorizeMock.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveAuthorization = resolve;
        }),
    );

    render(() => (
      <>
        <KonasePanel spaceId={spaceId()} />
        <button type="button" onClick={() => setSpaceId("space-b")}>
          Switch Space
        </button>
      </>
    ));

    fireEvent.input(screen.getByLabelText("Model API key"), {
      target: { value: "model-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect Ugoite MCP" }));
    await waitFor(() => expect(authorizeMock).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: "Switch Space" }));
    resolveAuthorization({
      accessToken: "space-a-token",
      endpoint: "/mcp",
      resource: `${location.origin}/mcp`,
      spaceUid: "space-a-uid",
    });

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Connect Ugoite MCP" }))
        .toBeInTheDocument()
    );
    expect(screen.queryByPlaceholderText(/Ask Konase/)).not.toBeInTheDocument();
  });
});
