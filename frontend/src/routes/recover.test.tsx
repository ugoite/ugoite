import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import RecoverRoute from "./recover";
import { authApi } from "~/lib/auth-api";

vi.mock("@solidjs/router", () => ({
  useSearchParams: () => [{ owner_approval_token: "owner-token" }],
  useNavigate: () => vi.fn(),
}));

vi.mock("~/lib/auth-api", () => ({
  authApi: { recoverSpaceAccess: vi.fn() },
}));

describe("/recover continuation", () => {
  it("starts supported Space access recovery with the owner token", async () => {
    vi.mocked(authApi.recoverSpaceAccess).mockResolvedValue({
      account: { account_id: "new-account", display_name: "Recovered" },
      recovery_codes: ["CODE-1"],
      audit_status: "delivered",
    });
    render(() => <RecoverRoute />);
    fireEvent.click(await screen.findByRole("button", { name: "Continue" }));
    await waitFor(() =>
      expect(authApi.recoverSpaceAccess).toHaveBeenCalledWith("owner-token")
    );
    expect(await screen.findByText("Save your new recovery codes"))
      .toBeInTheDocument();
  });
});
