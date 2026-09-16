import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import StepUpApprovalRoute from "./step-up";
import { authApi } from "~/lib/auth-api";
import {
  protocolFetch,
  UgoiteApiError,
} from "~/lib/ugoite-client/protocol";

const navigateMock = vi.fn();
const searchParams = vi.fn(() => [{ challenge: "challenge-1" }]);

vi.mock("@solidjs/router", () => ({
  useNavigate: () => navigateMock,
  useSearchParams: () => searchParams(),
}));

vi.mock("~/lib/auth-api", () => ({
  authApi: {
    getSession: vi.fn(),
    loginWithPasskey: vi.fn(),
  },
}));

vi.mock("~/lib/ugoite-client/protocol", () => ({
  protocolFetch: vi.fn(),
  UgoiteApiError: class UgoiteApiError extends Error {
    readonly code?: string;
    readonly status?: number;
    constructor(error: { message: string; code?: string; status?: number }) {
      super(error.message);
      this.name = "UgoiteApiError";
      this.code = error.code;
      this.status = error.status;
    }
  },
}));

describe("/step-up", () => {
  beforeEach(() => {
    navigateMock.mockReset();
    searchParams.mockReturnValue([{ challenge: "challenge-1" }]);
    vi.mocked(authApi.getSession).mockReset();
    vi.mocked(authApi.loginWithPasskey).mockReset();
    vi.mocked(protocolFetch).mockReset();
  });

  it("asks for the full link when the challenge is missing", async () => {
    searchParams.mockReturnValue([{}]);
    render(() => <StepUpApprovalRoute />);

    expect(await screen.findByText("This step-up link is incomplete.", {
      exact: false,
    })).toBeInTheDocument();
    expect(authApi.getSession).not.toHaveBeenCalled();
  });

  it("requires sign-in before showing the approval", async () => {
    vi.mocked(authApi.getSession).mockResolvedValue({ authenticated: false });
    render(() => <StepUpApprovalRoute />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Sign in with a passkey" }),
    );
    expect(navigateMock).toHaveBeenCalledWith(
      "/login?next=%2Fstep-up%3Fchallenge%3Dchallenge-1",
      { replace: true },
    );
    expect(protocolFetch).not.toHaveBeenCalled();
  });

  it("approves a pending challenge with a fresh passkey ceremony", async () => {
    vi.mocked(authApi.getSession).mockResolvedValue({ authenticated: true });
    vi.mocked(protocolFetch)
      .mockResolvedValueOnce({ status: "pending" })
      .mockResolvedValueOnce({ status: "approved" });
    vi.mocked(authApi.loginWithPasskey).mockResolvedValue();

    render(() => <StepUpApprovalRoute />);

    const approveButton = await screen.findByRole("button", {
      name: "Approve with a passkey",
    });
    await waitFor(() => expect(approveButton).toBeEnabled());
    fireEvent.click(approveButton);
    expect(await screen.findByText("Step-up approved.", { exact: false }))
      .toBeInTheDocument();
    expect(authApi.loginWithPasskey).toHaveBeenCalled();
    expect(protocolFetch).toHaveBeenCalledWith(
      "auth.step_up.status",
      { challenge_id: "challenge-1" },
    );
    expect(protocolFetch).toHaveBeenCalledWith(
      "auth.step_up.approve",
      {},
      { challenge_id: "challenge-1" },
    );
  });

  it("reports an already-used challenge without exposing secrets", async () => {
    vi.mocked(authApi.getSession).mockResolvedValue({ authenticated: true });
    vi.mocked(protocolFetch).mockRejectedValue(
      new UgoiteApiError({
        message: "unknown step-up challenge",
        code: "STEP_UP_NOT_FOUND",
        status: 404,
      }),
    );
    render(() => <StepUpApprovalRoute />);

    expect(await screen.findByText("already used or is unknown", {
      exact: false,
    })).toBeInTheDocument();
    expect(document.body.textContent).not.toContain("challenge-1");
  });

  it("reports an expired challenge", async () => {
    vi.mocked(authApi.getSession).mockResolvedValue({ authenticated: true });
    vi.mocked(protocolFetch).mockResolvedValue({ status: "expired" });
    render(() => <StepUpApprovalRoute />);

    expect(await screen.findByText("has expired", { exact: false }))
      .toBeInTheDocument();
  });
});
