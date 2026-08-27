import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import LoginRoute from "./login";
import { authApi } from "~/lib/auth-api";

const navigateMock = vi.fn();

vi.mock("@solidjs/router", () => ({
  A: (props: { children: unknown }) => <>{props.children}</>,
  useNavigate: () => navigateMock,
  useSearchParams: () => [{ next: "/spaces/demo/dashboard?tab=recent" }],
}));

vi.mock("~/lib/auth-api", () => ({
  authApi: {
    getConfig: vi.fn(),
    listOidcProviders: vi.fn(),
    loginWithPasskey: vi.fn(),
    loginWithOidc: vi.fn(),
  },
  oidcIssuerLabel: (issuer: string) => new URL(issuer).host,
}));

describe("/login continuation", () => {
  beforeEach(() => {
    navigateMock.mockReset();
    vi.mocked(authApi.getConfig).mockResolvedValue({
      status: "active",
      nodeId: "node",
      issuer: "http://localhost:3000",
      rpId: "localhost",
      passkey: true,
      oidc: false,
    });
    vi.mocked(authApi.listOidcProviders).mockResolvedValue([]);
    vi.mocked(authApi.loginWithPasskey).mockResolvedValue();
    vi.mocked(authApi.loginWithOidc).mockReset();
  });

  it("keeps the requested route after Passkey login", async () => {
    render(() => <LoginRoute />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Sign in with a passkey" }),
    );

    await waitFor(() =>
      expect(navigateMock).toHaveBeenCalledWith(
        "/spaces/demo/dashboard?tab=recent",
        { replace: true },
      )
    );
  });

  it("shows configured OIDC login after the Passkey primary action", async () => {
    vi.mocked(authApi.getConfig).mockResolvedValue({
      status: "active",
      nodeId: "node",
      issuer: "http://localhost:3000",
      rpId: "localhost",
      passkey: true,
      oidc: true,
    });
    vi.mocked(authApi.listOidcProviders).mockResolvedValue([{
      provider_id: "provider-1",
      issuer: "https://issuer.example/tenant-a",
      client_id: "client",
    }]);
    render(() => <LoginRoute />);

    fireEvent.click(
      await screen.findByRole("button", {
        name: "Continue with issuer.example",
      }),
    );

    expect(authApi.loginWithOidc).toHaveBeenCalledWith(
      "provider-1",
      undefined,
      "/spaces/demo/dashboard?tab=recent",
    );
  });

  it("does not expose OIDC when no provider is configured", async () => {
    render(() => <LoginRoute />);

    await screen.findByRole("button", { name: "Sign in with a passkey" });
    expect(screen.queryByText("Continue with issuer.example"))
      .toBeNull();
    expect(authApi.loginWithOidc).not.toHaveBeenCalled();
  });

  it("links to the dedicated Account Self-Recovery journey", async () => {
    render(() => <LoginRoute />);

    const link = await screen.findByRole("link", {
      name: "Lost your Passkey?",
    });
    expect(link).toHaveAttribute(
      "href",
      "/recover/account?next=%2Fspaces%2Fdemo%2Fdashboard%3Ftab%3Drecent",
    );
  });
});
