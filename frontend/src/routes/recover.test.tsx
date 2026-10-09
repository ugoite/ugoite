import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import RecoverRoute from "./recover/index";
import { authApi } from "~/lib/auth-api";
import { setLocale } from "~/lib/i18n";

const { navigateMock, searchParams } = vi.hoisted(() => ({
  navigateMock: vi.fn(),
  searchParams: {
    owner_approval_token: "owner-token",
    next: "/spaces/demo",
  } as Record<string, string>,
}));

vi.mock("@solidjs/router", () => ({
  useSearchParams: () => [searchParams],
  useNavigate: () => navigateMock,
}));

vi.mock("~/lib/auth-api", () => ({
  authApi: { recoverSpaceAccess: vi.fn() },
}));

describe("/recover owner-approved Space access", () => {
  beforeEach(() => {
    searchParams.owner_approval_token = "owner-token";
    searchParams.token = "";
    searchParams.next = "/spaces/demo";
    navigateMock.mockReset();
    vi.mocked(authApi.recoverSpaceAccess).mockReset();
    setLocale("en");
  });

  it("REQ-UX-RECOVERY-001: masks the owner token and names the action", () => {
    render(() => <RecoverRoute />);

    expect(screen.getByLabelText("Owner recovery token")).toHaveAttribute(
      "type",
      "password",
    );
    expect(screen.getByLabelText("Owner recovery token")).toHaveValue(
      "owner-token",
    );
    expect(screen.getByRole("button", { name: "Continue" }))
      .toBeInTheDocument();
  });

  it("REQ-UX-RECOVERY-001: localizes labels and recovery states", async () => {
    setLocale("ja");
    vi.mocked(authApi.recoverSpaceAccess).mockResolvedValue({
      account: { account_id: "private-account-id", display_name: "Recovered" },
      recovery_codes: ["CODE-1"],
      audit_status: "delivered",
    });
    render(() => <RecoverRoute />);

    fireEvent.click(screen.getByRole("button", { name: "続行" }));

    await waitFor(() =>
      expect(authApi.recoverSpaceAccess).toHaveBeenCalledWith("owner-token")
    );
    expect(
      await screen.findByRole("heading", {
        name: "新しいリカバリーコードを保存",
      }),
    ).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(
      "回復の監査記録を保存しました。",
    );
    expect(screen.queryByText("delivered")).not.toBeInTheDocument();
    expect(
      screen.getByText(
        "コードはオフラインで保管してください。一度だけ表示されます。",
      ),
    )
      .toBeInTheDocument();
  });

  it("REQ-UX-RECOVERY-001: hides returned internal identifiers", async () => {
    vi.mocked(authApi.recoverSpaceAccess).mockResolvedValue({
      account: { account_id: "private-account-id", display_name: "Recovered" },
      recovery_codes: ["CODE-1"],
      audit_status: "pending",
    });
    render(() => <RecoverRoute />);

    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByText("CODE-1");

    expect(screen.queryByText("private-account-id")).not.toBeInTheDocument();
    expect(screen.queryByText("Recovered")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Recovery audit is still being recorded.",
    );
    expect(screen.queryByText("pending")).not.toBeInTheDocument();
  });

  it("REQ-UX-RECOVERY-001: hides server error details", async () => {
    vi.mocked(authApi.recoverSpaceAccess).mockRejectedValue(
      new Error("Internal failure for account private-account-id"),
    );
    render(() => <RecoverRoute />);

    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Space access recovery failed.",
    );
    expect(screen.queryByText(/private-account-id/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Internal failure/)).not.toBeInTheDocument();
  });

  it("REQ-UX-RECOVERY-001: acknowledges one-time codes before safe continuation", async () => {
    vi.mocked(authApi.recoverSpaceAccess).mockResolvedValue({
      account: { account_id: "private-account-id", display_name: "Recovered" },
      recovery_codes: ["CODE-1"],
      audit_status: "delivered",
    });
    render(() => <RecoverRoute />);

    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByText("CODE-1");
    fireEvent.click(screen.getByRole("button", { name: "I saved the codes" }));

    expect(navigateMock).toHaveBeenCalledWith("/spaces/demo", {
      replace: true,
    });
  });
});
