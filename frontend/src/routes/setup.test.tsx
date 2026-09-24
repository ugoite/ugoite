import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SetupRoute from "./setup";
import { authApi } from "~/lib/auth-api";

const navigateMock = vi.fn();

vi.mock("@solidjs/router", () => ({
  useNavigate: () => navigateMock,
  useSearchParams: () => [{ next: "/spaces/demo/dashboard?tab=recent" }],
}));

vi.mock("~/lib/auth-api", () => ({
  authApi: {
    getConfig: vi.fn(),
    getSession: vi.fn(),
    setup: vi.fn(),
    addPasskey: vi.fn(),
  },
}));

describe("/setup continuation", () => {
  beforeEach(() => {
    navigateMock.mockReset();
    vi.mocked(authApi.getConfig).mockResolvedValue({
      status: "uninitialized",
      nodeId: "node",
      issuer: "http://localhost:3000",
      rpId: "localhost",
      passkey: true,
      oidc: false,
    });
    vi.mocked(authApi.getSession).mockResolvedValue({ authenticated: false });
    vi.mocked(authApi.setup).mockResolvedValue({
      recovery_codes: ["recovery-code"],
      account: { account_id: "account", display_name: "Admin" },
    });
    vi.mocked(authApi.addPasskey).mockResolvedValue();
  });

  it("keeps the protected route after setup strengthening", async () => {
    render(() => <SetupRoute />);

    fireEvent.input(screen.getByLabelText("Setup secret"), {
      target: { value: "setup-secret" },
    });
    fireEvent.input(screen.getByLabelText("Display name"), {
      target: { value: "Admin" },
    });
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Create administrator passkey",
      }),
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Register second Passkey" }))
        .toBeInTheDocument()
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Register second Passkey" }),
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Continue" }))
        .toBeInTheDocument()
    );

    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(navigateMock).toHaveBeenCalledWith(
      "/spaces/demo/dashboard?tab=recent",
      { replace: true },
    );
  });
});
