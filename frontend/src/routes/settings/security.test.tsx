import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SecuritySettingsRoute from "./security";

vi.mock("~/components/GlobalShell", () => ({
  GlobalShell: (props: { children: unknown }) => <div>{props.children}</div>,
}));

vi.mock("~/lib/auth-api", () => ({
  authApi: {
    listPasskeys: vi.fn(),
    listSessions: vi.fn(),
    listDevices: vi.fn(),
    listOidcProviders: vi.fn(),
  },
}));

describe("SecuritySettingsRoute", () => {
  beforeEach(async () => {
    const { authApi } = await import("~/lib/auth-api");
    vi.mocked(authApi.listPasskeys).mockResolvedValue([]);
    vi.mocked(authApi.listSessions).mockResolvedValue([]);
    vi.mocked(authApi.listDevices).mockResolvedValue([]);
    vi.mocked(authApi.listOidcProviders).mockResolvedValue([]);
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
  });
});
