import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import LoginRoute from "./login";
import { authApi } from "~/lib/auth-api";
import { setLocale } from "~/lib/i18n";

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
    setLocale("en");
    vi.mocked(authApi.getConfig).mockReset();
    vi.mocked(authApi.getConfig).mockResolvedValue({
      status: "active",
      nodeId: "node",
      issuer: "http://localhost:3000",
      rpId: "localhost",
      passkey: true,
      oidc: false,
    });
    vi.mocked(authApi.listOidcProviders).mockReset();
    vi.mocked(authApi.listOidcProviders).mockResolvedValue([]);
    vi.mocked(authApi.loginWithPasskey).mockReset();
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

  it("REQ-FE-069: sends first-run authentication to setup", async () => {
    vi.mocked(authApi.getConfig).mockResolvedValue({
      status: "uninitialized",
      nodeId: "node",
      issuer: "http://localhost:3000",
      rpId: "localhost",
      passkey: true,
      oidc: false,
    });
    render(() => <LoginRoute />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Sign in with a passkey" }),
    );

    await waitFor(() =>
      expect(navigateMock).toHaveBeenCalledWith(
        "/setup?next=%2Fspaces%2Fdemo%2Fdashboard%3Ftab%3Drecent",
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

  it("REQ-UX-RESP-001: exposes a single sign-in task with one inline error", async () => {
    vi.mocked(authApi.loginWithPasskey).mockRejectedValue(
      new Error("No passkey found"),
    );
    render(() => <LoginRoute />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Sign in with a passkey" }),
    );

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Sign-in failed.");
    // A single task surface: one error, one primary action, one recovery path.
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(
      screen.getByRole("button", { name: "Try again" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Lost your Passkey?" }))
      .toBeInTheDocument();
    expect(screen.getByText("No passkey found", { selector: "pre" }))
      .not.toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() =>
      expect(authApi.loginWithPasskey).toHaveBeenCalledTimes(2)
    );
  });

  it("REQ-FE-069: renders only the logo and authentication actions", async () => {
    render(() => <LoginRoute />);

    expect(await screen.findByRole("heading", { name: "Ugoite" }))
      .toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /sign in/i }))
      .toHaveLength(1);
    expect(document.querySelectorAll(".loginPanel > .btn.primary"))
      .toHaveLength(1);
    expect(document.querySelector(".loginStatement")).toBeNull();
    expect(document.querySelectorAll(".loginPanel > *")).toHaveLength(3);
  });

  it("REQ-FE-069: lets users retry when authentication options fail to load", async () => {
    vi.mocked(authApi.getConfig)
      .mockRejectedValueOnce(new Error("network unavailable"))
      .mockResolvedValueOnce({
        status: "active",
        nodeId: "node",
        issuer: "http://localhost:3000",
        rpId: "localhost",
        passkey: true,
        oidc: false,
      });
    render(() => <LoginRoute />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Sign-in options are unavailable.",
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(
      await screen.findByRole("button", {
        name: "Sign in with a passkey",
      }),
    ).toBeInTheDocument();
    expect(authApi.getConfig).toHaveBeenCalledTimes(2);
  });

  it("autofocuses the primary sign-in action once the config loads", async () => {
    render(() => <LoginRoute />);

    const signIn = await screen.findByRole("button", {
      name: "Sign in with a passkey",
    });
    await waitFor(() => expect(signIn).toHaveFocus());
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
