import { render, waitFor } from "@solidjs/testing-library";
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
  it("opens Login for a signed-out or unavailable session", async () => {
    getSession.mockResolvedValue({ authenticated: false });
    render(() => <IndexRoute />);
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith("/login", { replace: true })
    );
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
