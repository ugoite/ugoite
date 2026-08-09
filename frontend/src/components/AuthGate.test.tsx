import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AuthGate } from "./AuthGate";
import { authApi } from "~/lib/auth-api";

const navigateMock = vi.fn();
const [pathname, setPathname] = createSignal("/spaces/demo/dashboard");
const [search, setSearch] = createSignal("?tab=recent");
const locationMock = {
  get pathname() {
    return pathname();
  },
  get search() {
    return search();
  },
};

vi.mock("@solidjs/router", () => ({
  useLocation: () => locationMock,
  useNavigate: () => navigateMock,
}));

vi.mock("~/lib/auth-api", () => ({
  authApi: {
    getSession: vi.fn(),
  },
}));

describe("AuthGate", () => {
  beforeEach(() => {
    setPathname("/spaces/demo/dashboard");
    setSearch("?tab=recent");
    sessionStorage.clear();
    navigateMock.mockReset();
    vi.mocked(authApi.getSession).mockReset();
  });

  it("does not render protected content while the session is pending", async () => {
    let resolveSession: (value: { authenticated: boolean }) => void = () => {};
    vi.mocked(authApi.getSession).mockReturnValue(
      new Promise((resolve) => {
        resolveSession = resolve;
      }),
    );

    render(() => (
      <AuthGate>
        <p>Protected dashboard</p>
      </AuthGate>
    ));

    expect(screen.queryByText("Protected dashboard")).not.toBeInTheDocument();
    expect(screen.getByText("Checking authentication…")).toBeInTheDocument();

    resolveSession({ authenticated: false });
    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith(
        "/login?next=%2Fspaces%2Fdemo%2Fdashboard%3Ftab%3Drecent",
        { replace: true },
      );
    });
    expect(screen.queryByText("Protected dashboard")).not.toBeInTheDocument();
  });

  it("renders protected content only after an authenticated session", async () => {
    vi.mocked(authApi.getSession).mockResolvedValue({ authenticated: true });

    render(() => (
      <AuthGate>
        <p>Protected dashboard</p>
      </AuthGate>
    ));

    expect(screen.queryByText("Protected dashboard")).not.toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByText("Protected dashboard")).toBeInTheDocument()
    );
    expect(navigateMock).not.toHaveBeenCalled();
  });

  it("redirects when the session request is rejected", async () => {
    vi.mocked(authApi.getSession).mockRejectedValue(new Error("offline"));

    render(() => (
      <AuthGate>
        <p>Protected dashboard</p>
      </AuthGate>
    ));

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith(
        "/login?next=%2Fspaces%2Fdemo%2Fdashboard%3Ftab%3Drecent",
        { replace: true },
      );
    });
  });

  it("does not let a stale protected check redirect after a public navigation", async () => {
    let resolveSession: (value: { authenticated: boolean }) => void = () => {};
    vi.mocked(authApi.getSession).mockReturnValue(
      new Promise((resolve) => {
        resolveSession = resolve;
      }),
    );

    render(() => (
      <AuthGate>
        <p>Invitation join</p>
      </AuthGate>
    ));

    setPathname("/about");
    setSearch("");
    expect(screen.getByText("Invitation join")).toBeInTheDocument();

    resolveSession({ authenticated: false });
    await Promise.resolve();
    expect(navigateMock).not.toHaveBeenCalled();
  });

  it("leaves public routes available without a session request", () => {
    setPathname("/spaces/join");
    setSearch("");

    render(() => (
      <AuthGate>
        <p>Invitation join</p>
      </AuthGate>
    ));

    expect(screen.getByText("Invitation join")).toBeInTheDocument();
    expect(authApi.getSession).not.toHaveBeenCalled();
    expect(navigateMock).not.toHaveBeenCalled();
  });
});
