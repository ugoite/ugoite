import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import RecoverRoute from "./recover";
import { authApi } from "~/lib/auth-api";

const navigateMock = vi.fn();

vi.mock("@solidjs/router", () => ({
  useNavigate: () => navigateMock,
  useSearchParams: () => [{ next: "/spaces/demo/dashboard?tab=recent" }],
}));

vi.mock("~/lib/auth-api", () => ({
  authApi: {
    recoverPasskey: vi.fn(),
  },
}));

describe("/recover continuation", () => {
  beforeEach(() => {
    navigateMock.mockReset();
    vi.mocked(authApi.recoverPasskey).mockResolvedValue({
      recovery_codes: ["replacement-code"],
    });
  });

  it("keeps the requested route after recovery", async () => {
    render(() => <RecoverRoute />);

    fireEvent.input(screen.getByLabelText("Account ID"), {
      target: { value: "account" },
    });
    fireEvent.input(screen.getByLabelText("Recovery code"), {
      target: { value: "recovery" },
    });
    fireEvent.input(screen.getByLabelText("TOTP code"), {
      target: { value: "123456" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Register replacement Passkey" }),
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "I saved the codes" }),
    );

    await waitFor(() =>
      expect(navigateMock).toHaveBeenCalledWith(
        "/spaces/demo/dashboard?tab=recent",
        { replace: true },
      )
    );
  });
});
