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

  it("does not expose future OIDC login in the v0.1 browser flow", async () => {
    render(() => <LoginRoute />);

    await screen.findByRole("button", { name: "Sign in with a passkey" });
    expect(screen.queryByText("Sign in with https://issuer.example"))
      .toBeNull();
    expect(authApi.loginWithOidc).not.toHaveBeenCalled();
  });
});
