import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GlobalShell } from "./GlobalShell";
import { authApi } from "~/lib/ugoite-client";
import { setLocale } from "~/lib/i18n";

vi.mock("~/lib/ugoite-client", () => ({
  authApi: {
    clearSession: vi.fn(),
  },
}));

describe("GlobalShell account menu", () => {
  beforeEach(() => {
    setLocale("en");
    vi.mocked(authApi.clearSession).mockReset();
  });

  it("does not sign out when the avatar is opened", () => {
    render(() => (
      <GlobalShell title="Spaces">
        <p>Content</p>
      </GlobalShell>
    ));

    fireEvent.click(screen.getByRole("button", { name: "Account" }));

    expect(screen.getByRole("menu")).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Account settings" }))
      .toHaveAttribute("href", "/settings/security");
    expect(authApi.clearSession).not.toHaveBeenCalled();
  });

  it("signs out only from the explicit menu action", async () => {
    vi.mocked(authApi.clearSession).mockResolvedValue(undefined);
    render(() => (
      <GlobalShell title="Spaces">
        <p>Content</p>
      </GlobalShell>
    ));

    fireEvent.click(screen.getByRole("button", { name: "Account" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Sign out" }));

    expect(authApi.clearSession).toHaveBeenCalledOnce();
  });

  it("shows a sign-in link when used for a public route", () => {
    render(() => (
      <GlobalShell title="About" authenticated={false}>
        <p>Content</p>
      </GlobalShell>
    ));

    expect(screen.getByRole("link", { name: "Sign in" })).toHaveAttribute(
      "href",
      "/login",
    );
    expect(screen.queryByRole("button", { name: "Account" })).toBeNull();
  });
});
