import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
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
    addPasskey: vi.fn(),
    revokePasskey: vi.fn(),
    revokeSession: vi.fn(),
    revokeDevice: vi.fn(),
    linkOidc: vi.fn(),
    startTotpEnrollment: vi.fn(),
    finishTotpEnrollment: vi.fn(),
  },
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
    vi.mocked(authApi.addPasskey).mockResolvedValue(undefined);
    vi.mocked(authApi.revokePasskey).mockResolvedValue(undefined);
    vi.mocked(authApi.revokeSession).mockResolvedValue(undefined);
    vi.mocked(authApi.revokeDevice).mockResolvedValue(undefined);
    vi.mocked(authApi.linkOidc).mockImplementation(() => undefined);
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
    expect(screen.queryByRole("tabpanel", { name: "Recovery TOTP" }))
      .toBeNull();

    fireEvent.click(screen.getByRole("tab", { name: "Recovery TOTP" }));

    expect(screen.getByRole("tabpanel", { name: "Recovery TOTP" }))
      .toBeInTheDocument();
    expect(screen.queryByRole("tabpanel", { name: "Passkeys" })).toBeNull();
    expect(setSearchParams).toHaveBeenCalledWith({ tab: "totp" });
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
    fireEvent.click(await screen.findByRole("button", { name: "パスキーを追加" }));

    await screen.findByRole("alert");
    expect(screen.getByRole("alert")).toHaveTextContent(
      "TOTP コードが正しくありません。",
    );
    expect(screen.getByRole("alert")).toHaveTextContent("credential-1");
  });

  it("localizes a client-side passkey cancellation", async () => {
    setLocale("ja");
    vi.mocked(authApi.addPasskey).mockRejectedValue(new UgoiteApiError({
      kind: "cancelled",
      code: "PASSKEY_CANCELLED",
      operation: "auth.passkey",
      message: "",
    }));

    render(() => <SecuritySettingsRoute />);
    fireEvent.click(await screen.findByRole("button", { name: "パスキーを追加" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("パスキーの操作をキャンセルしました。");
    expect(alert).not.toHaveTextContent("Passkey");
  });

  it("routes session, device, OIDC, and TOTP failures through the same action state", async () => {
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
});
