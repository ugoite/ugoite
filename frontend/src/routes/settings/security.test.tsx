import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SecuritySettingsRoute from "./security";

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
    listPasskeys: vi.fn(),
    listSessions: vi.fn(),
    listDevices: vi.fn(),
    listOidcProviders: vi.fn(),
  },
}));

describe("SecuritySettingsRoute", () => {
  beforeEach(async () => {
    for (const key of Object.keys(searchParams)) delete searchParams[key];
    setSearchParams.mockReset();
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
});
