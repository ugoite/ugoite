import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import AccountRecoveryRoute from "./account";
import { authApi } from "~/lib/auth-api";

const navigateMock = vi.fn();

vi.mock("@solidjs/router", () => ({
  useNavigate: () => navigateMock,
  useSearchParams: () => [{ next: "/spaces/demo" }],
}));

vi.mock("~/lib/auth-api", () => ({
  authApi: { recoverPasskey: vi.fn() },
}));

describe("/recover/account", () => {
  it("replaces the Passkey and displays the rotated codes once", async () => {
    vi.mocked(authApi.recoverPasskey).mockResolvedValue({
      account: { account_id: "account-1", display_name: "Recovered" },
      recovery_codes: ["NEW-CODE-1", "NEW-CODE-2"],
    });
    render(() => <AccountRecoveryRoute />);

    fireEvent.input(screen.getByLabelText("Account ID"), {
      target: { value: "account-1" },
    });
    fireEvent.input(screen.getByLabelText("Recovery Code"), {
      target: { value: "OLD-CODE-1" },
    });
    fireEvent.input(screen.getByLabelText("Authenticator code"), {
      target: { value: "123456" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Register new Passkey" }),
    );

    await waitFor(() =>
      expect(authApi.recoverPasskey).toHaveBeenCalledWith(
        "account-1",
        "OLD-CODE-1",
        "123456",
      )
    );
    expect(await screen.findByText("Save your new recovery codes"))
      .toBeInTheDocument();
    expect(screen.getByText("NEW-CODE-1")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "I saved the codes" }));
    expect(navigateMock).toHaveBeenCalledWith("/spaces/demo", {
      replace: true,
    });
  });
});
