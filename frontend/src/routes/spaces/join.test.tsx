import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SpaceInvitationJoinRoute from "./join";
import { authApi } from "~/lib/auth-api";

const navigateMock = vi.fn();

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; children: unknown }) => (
    <a href={props.href}>{props.children}</a>
  ),
  useNavigate: () => navigateMock,
}));

vi.mock("~/components/GlobalShell", () => ({
  GlobalShell: (props: { authenticated?: boolean; children: unknown }) => (
    <div data-authenticated={String(props.authenticated)}>{props.children}</div>
  ),
}));

vi.mock("~/lib/auth-api", () => ({
  authApi: {
    acceptInvitation: vi.fn(),
    getSession: vi.fn(),
    listOidcProviders: vi.fn(),
    loginWithOidc: vi.fn(),
    registerInvitation: vi.fn(),
  },
  oidcIssuerLabel: (issuer: string) => new URL(issuer).host,
}));

describe("/spaces/join", () => {
  beforeEach(() => {
    navigateMock.mockReset();
    vi.mocked(authApi.acceptInvitation).mockReset();
    vi.mocked(authApi.getSession).mockResolvedValue({ authenticated: false });
    vi.mocked(authApi.listOidcProviders).mockResolvedValue([]);
    vi.mocked(authApi.loginWithOidc).mockReset();
    vi.mocked(authApi.registerInvitation).mockReset();
  });

  it("renders as a public shell before an invitation recipient signs in", async () => {
    render(() => <SpaceInvitationJoinRoute />);

    await waitFor(() =>
      expect(screen.getByText("Join a Space").parentElement?.parentElement)
        .toHaveAttribute("data-authenticated", "false")
    );
  });

  it("accepts an invitation for an already signed-in account", async () => {
    vi.mocked(authApi.getSession).mockResolvedValue({ authenticated: true });
    vi.mocked(authApi.acceptInvitation).mockResolvedValue();
    render(() => <SpaceInvitationJoinRoute />);

    fireEvent.input(screen.getByLabelText("Invitation token"), {
      target: { value: "invitation-token" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Accept invitation" }));

    await waitFor(() => {
      expect(authApi.acceptInvitation).toHaveBeenCalledWith("invitation-token");
      expect(navigateMock).toHaveBeenCalledWith("/spaces", { replace: true });
    });
    expect(authApi.registerInvitation).not.toHaveBeenCalled();
  });

  it("starts OIDC login with the invitation token", async () => {
    vi.mocked(authApi.listOidcProviders).mockResolvedValue([{
      provider_id: "provider-1",
      issuer: "https://issuer.example",
      client_id: "client",
    }]);
    render(() => <SpaceInvitationJoinRoute />);

    fireEvent.input(screen.getByLabelText("Invitation token"), {
      target: { value: "invitation-token" },
    });
    fireEvent.click(
      await screen.findByRole("button", { name: "Continue with issuer.example" }),
    );

    expect(authApi.loginWithOidc).toHaveBeenCalledWith(
      "provider-1",
      "invitation-token",
    );
  });
});
