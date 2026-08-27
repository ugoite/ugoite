import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SecuritySettingsRoute from "./security";
import { authApi } from "~/lib/auth-api";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import { setLocale } from "~/lib/i18n";

const searchParams: Record<string, string> = {};
const setSearchParams = vi.fn();

vi.mock("@solidjs/router", () => ({
  useSearchParams: () => [searchParams, setSearchParams],
}));

vi.mock("~/components/GlobalShell", () => ({
  GlobalShell: (props: { children: unknown }) => <div>{props.children}</div>,
}));

vi.mock("~/lib/auth-api", () => ({
  authApi: {
    listAudit: vi.fn(),
    listPasskeys: vi.fn(),
    listSessions: vi.fn(),
    listDevices: vi.fn(),
    listOidcProviders: vi.fn(),
    listOidcLinks: vi.fn(),
    addPasskey: vi.fn(),
    revokePasskey: vi.fn(),
    revokeSession: vi.fn(),
    revokeDevice: vi.fn(),
    linkOidc: vi.fn(),
    unlinkOidc: vi.fn(),
    addBootstrapPasskey: vi.fn(),
    startTotpEnrollment: vi.fn(),
    finishTotpEnrollment: vi.fn(),
  },
  oidcIssuerLabel: (issuer: string) => issuer.replace(/^https?:\/\//, ""),
}));

vi.mock("~/components/AuditLogViewer", () => ({
  NodeAuditLogViewer: () => <div>Audit viewer</div>,
}));

describe("SecuritySettingsRoute", () => {
  beforeEach(async () => {
    setLocale("en");
    for (const key of Object.keys(searchParams)) delete searchParams[key];
    setSearchParams.mockReset();
    const { authApi } = await import("~/lib/auth-api");
    vi.mocked(authApi.listAudit).mockResolvedValue([]);
    vi.mocked(authApi.listPasskeys).mockResolvedValue([]);
    vi.mocked(authApi.listSessions).mockResolvedValue([]);
    vi.mocked(authApi.listDevices).mockResolvedValue([]);
    vi.mocked(authApi.listOidcProviders).mockResolvedValue([]);
    vi.mocked(authApi.listOidcLinks).mockResolvedValue([]);
    vi.mocked(authApi.addPasskey).mockResolvedValue(undefined);
    vi.mocked(authApi.revokePasskey).mockResolvedValue(undefined);
    vi.mocked(authApi.revokeSession).mockResolvedValue(undefined);
    vi.mocked(authApi.revokeDevice).mockResolvedValue(undefined);
    vi.mocked(authApi.linkOidc).mockImplementation(() => undefined);
    vi.mocked(authApi.unlinkOidc).mockResolvedValue(undefined);
    vi.mocked(authApi.addBootstrapPasskey).mockResolvedValue(undefined);
    vi.mocked(authApi.startTotpEnrollment).mockResolvedValue({
      secret: "secret",
      otpauth_uri: "otpauth://totp/test",
    });
    vi.mocked(authApi.finishTotpEnrollment).mockResolvedValue(undefined);
  });

  it("shows only the selected credential panel", () => {
    render(() => <SecuritySettingsRoute />);

    expect(screen.getByRole("tabpanel", { name: "Passkeys" }))
      .toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Recovery TOTP" })).toBeNull();
    expect(screen.queryByRole("tab", { name: "OIDC" })).toBeNull();
    expect(screen.queryByRole("tab", { name: "CLI / MCP" })).toBeNull();
  });

  it("opens the credential panel selected by the URL", () => {
    searchParams.tab = "sessions";
    render(() => <SecuritySettingsRoute />);

    expect(screen.getByRole("tabpanel", { name: "Sessions" }))
      .toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Sessions" }))
      .toHaveAttribute("aria-selected", "true");
  });

  it("exposes the node audit viewer from account security settings", async () => {
    searchParams.tab = "audit";
    render(() => <SecuritySettingsRoute />);

    expect(await screen.findByRole("tabpanel", { name: "Audit Log" }))
      .toBeInTheDocument();
    expect(screen.getByText("Audit viewer")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Audit Log" }))
      .toHaveAttribute("aria-selected", "true");
  });

  it("shows one localized error for passkey actions", async () => {
    setLocale("ja");
    const error = new UgoiteApiError({
      kind: "invalid_arguments",
      code: "INVALID_TOTP",
      status: 422,
      message: "invalid credential",
      detail: { request_id: "credential-1" },
    });
    vi.mocked(authApi.addPasskey).mockRejectedValue(error);

    render(() => <SecuritySettingsRoute />);
    fireEvent.click(
      await screen.findByRole("button", { name: "パスキーを追加" }),
    );

    await screen.findByRole("alert");
    expect(screen.getByRole("alert")).toHaveTextContent(
      "TOTP コードが正しくありません。",
    );
    expect(screen.getByRole("alert")).toHaveTextContent("credential-1");
  });

  it("sets up the recovery-only authenticator without adding a login method", async () => {
    render(() => <SecuritySettingsRoute />);

    fireEvent.click(
      await screen.findByRole("button", {
        name: "Set up or replace recovery authenticator",
      }),
    );
    expect(await screen.findByTestId("recovery-secret")).toHaveTextContent(
      "secret",
    );
    fireEvent.input(screen.getByLabelText("Current six-digit code"), {
      target: { value: "123456" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Confirm TOTP" }));

    await waitFor(() =>
      expect(authApi.finishTotpEnrollment).toHaveBeenCalledWith("123456")
    );
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Recovery TOTP configured.",
    );
  });

  it("localizes a client-side passkey cancellation", async () => {
    setLocale("ja");
    vi.mocked(authApi.addPasskey).mockRejectedValue(
      new UgoiteApiError({
        kind: "cancelled",
        code: "PASSKEY_CANCELLED",
        operation: "auth.passkey",
        message: "",
      }),
    );

    render(() => <SecuritySettingsRoute />);
    fireEvent.click(
      await screen.findByRole("button", { name: "パスキーを追加" }),
    );

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("パスキーの操作をキャンセルしました。");
    expect(alert).not.toHaveTextContent("Passkey");
  });

  it("falls back to a supported panel for a future tab URL", () => {
    searchParams.tab = "totp";
    render(() => <SecuritySettingsRoute />);

    expect(screen.getByRole("tabpanel", { name: "Passkeys" }))
      .toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Passkeys" }))
      .toHaveAttribute("aria-selected", "true");
  });

  it("routes session failures through the same action state", async () => {
    setLocale("ja");
    const failure = new UgoiteApiError({
      kind: "forbidden",
      code: "FORBIDDEN",
      status: 403,
      message: "forbidden",
    });

    searchParams.tab = "sessions";
    vi.mocked(authApi.listSessions).mockResolvedValue([
      { session_id: "session-1", last_seen_at: "2026-01-01T00:00:00Z" },
    ]);
    vi.mocked(authApi.revokeSession).mockRejectedValue(failure);
    render(() => <SecuritySettingsRoute />);
    fireEvent.click(await screen.findByRole("button", { name: "取り消し" }));
    await screen.findByRole("alert");
    expect(screen.getByRole("alert")).toHaveTextContent(
      "この操作を行う権限がありません。",
    );
  });

  it("lists OIDC links and keeps linking actions behind the security settings", async () => {
    vi.mocked(authApi.listOidcProviders).mockResolvedValue([]);
    vi.mocked(authApi.listOidcLinks).mockResolvedValue([{
      method_id: "method-1",
      issuer: "https://id.example/tenant-a",
      created_at: "2026-01-01T00:00:00Z",
      last_used_at: null,
    }]);

    render(() => <SecuritySettingsRoute />);

    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(screen.getByText("External sign-ins")).toBeInTheDocument();
    expect(screen.getByText("id.example/tenant-a", { exact: false }))
      .toBeInTheDocument();
    (screen.getByRole("button", { name: "Unlink" }) as HTMLButtonElement)
      .click();
    await waitFor(() =>
      expect(authApi.unlinkOidc).toHaveBeenCalledWith("method-1")
    );
  });

  it("offers the first-Passkey bootstrap only on the invitation callback journey", async () => {
    searchParams.bootstrap = "1";
    vi.mocked(authApi.addBootstrapPasskey).mockResolvedValue(undefined);

    render(() => <SecuritySettingsRoute />);

    await new Promise((resolve) => setTimeout(resolve, 10));
    (
      screen.getByRole("button", { name: "Register first Passkey" }) as
        HTMLButtonElement
    ).click();
    await waitFor(() =>
      expect(authApi.addBootstrapPasskey).toHaveBeenCalledTimes(1)
    );
  });
});
