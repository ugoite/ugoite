import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import IndexRoute from "./index";
const navigate = vi.fn();
const getSession = vi.fn();
vi.mock("@solidjs/router", () => ({ useNavigate: () => navigate }));
vi.mock(
  "~/lib/ugoite-client",
  () => ({
    authApi: { getSession: (...args: unknown[]) => getSession(...args) },
  }),
);
describe("root route", () => {
  beforeEach(() => {
    navigate.mockReset();
    getSession.mockReset();
  });
  it("opens Spaces for an authenticated session", async () => {
    getSession.mockResolvedValue({ authenticated: true });
    render(() => <IndexRoute />);
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith("/spaces", { replace: true })
    );
  });
  it("REQ-FE-069: keeps unauthenticated visitors on the login entry", async () => {
    let resolveSession: (value: { authenticated: boolean }) => void = () => {};
    getSession.mockReturnValue(
      new Promise((resolve) => {
        resolveSession = resolve;
      }),
    );
    render(() => <IndexRoute />);

    expect(screen.getByRole("heading", { name: "Ugoite" }))
      .toBeInTheDocument();
    expect(screen.getByRole("status")).toBeInTheDocument();

    const login = screen.getByRole("link", { name: "Login" });
    await waitFor(() => expect(login).toHaveFocus());
    expect(login).toHaveAttribute("href", "/login");
    expect(login).toHaveClass("btn", "primary");
    expect(document.querySelectorAll(".loginPanel > .btn.primary"))
      .toHaveLength(1);
    expect(document.querySelectorAll(".loginPanel > *")).toHaveLength(3);

    resolveSession({ authenticated: false });
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());

    await waitFor(() => expect(getSession).toHaveBeenCalledTimes(1));
    expect(navigate).not.toHaveBeenCalled();
  });
  it("does not navigate after the route is unmounted while checking", async () => {
    let resolveSession: (value: { authenticated: boolean }) => void = () => {};
    getSession.mockReturnValue(
      new Promise((resolve) => {
        resolveSession = resolve;
      }),
    );
    const { unmount } = render(() => <IndexRoute />);

    unmount();
    resolveSession({ authenticated: false });
    await Promise.resolve();
    expect(navigate).not.toHaveBeenCalled();
  });
});
