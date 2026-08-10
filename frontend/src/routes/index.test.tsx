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
  it("REQ-OPS-015: keeps signed-out visitors on the public landing page", async () => {
    getSession.mockResolvedValue({ authenticated: false });
    render(() => <IndexRoute />);

    expect(screen.getByRole("heading", { name: "Ugoite" }))
      .toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Log in" })).toHaveAttribute(
      "href",
      "/login",
    );
    expect(screen.getByRole("link", { name: "Log in" })).toHaveClass(
      "ui-button-primary",
    );
    expect(screen.getByRole("link", { name: "Open Spaces" })).toHaveAttribute(
      "href",
      "/spaces",
    );
    expect(screen.getByRole("link", { name: "Open Spaces" })).toHaveClass(
      "ui-button-secondary",
    );
    expect(screen.getByRole("link", { name: "Learn More" })).toHaveAttribute(
      "href",
      "https://ugoite.github.io/ugoite/docs/guide/start",
    );
    expect(
      screen.getByText(/\/spaces requires an authenticated browser session/),
    )
      .toBeInTheDocument();

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
