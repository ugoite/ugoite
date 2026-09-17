import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, within } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GlobalShell } from "./GlobalShell";
import { authApi } from "~/lib/ugoite-client";
import { setLocale } from "~/lib/i18n";

const docsHref =
  "https://ugoite.github.io/ugoite/docs/guide/start";

vi.mock("~/lib/ugoite-client", () => ({
  authApi: {
    clearSession: vi.fn(),
  },
}));

vi.mock("@solidjs/router", () => ({
  A: (props: Record<string, unknown>) => {
    const { children, ...rest } = props;
    return <a {...(rest as never)}>{children as never}</a>;
  },
  useNavigate: () => vi.fn(),
  useParams: () => ({}),
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

    const menu = screen.getByRole("menu");
    expect(menu).toBeInTheDocument();
    expect(within(menu).getByRole("menuitem", { name: "Account settings" }))
      .toHaveAttribute("href", "/settings/security");
    expect(authApi.clearSession).not.toHaveBeenCalled();
  });

  it("offers Settings, Docs, and Logout without a menu heading", () => {
    render(() => (
      <GlobalShell title="Spaces">
        <p>Content</p>
      </GlobalShell>
    ));

    fireEvent.click(screen.getByRole("button", { name: "Account" }));

    const menu = screen.getByRole("menu");
    const items = within(menu).getAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual([
      "Account settings",
      "Docs",
      "Sign out",
    ]);
    expect(
      within(menu).getByRole("menuitem", { name: "Docs" }),
    ).toHaveAttribute("href", docsHref);
    expect(
      within(menu).getByRole("menuitem", { name: "Docs" }),
    ).toHaveAttribute("target", "_blank");
    expect(within(menu).queryByText("Account")).not.toBeInTheDocument();
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
      <GlobalShell title="Spaces" authenticated={false}>
        <p>Content</p>
      </GlobalShell>
    ));

    expect(screen.getByRole("link", { name: "Sign in" })).toHaveAttribute(
      "href",
      "/login",
    );
    expect(screen.queryByRole("button", { name: "Account" })).toBeNull();
  });

  it("REQ-UX-NAV-001: keeps global navigation limited to Spaces without an About entry", () => {
    render(() => (
      <GlobalShell title="Spaces">
        <p>Content</p>
      </GlobalShell>
    ));

    expect(screen.getAllByRole("link", { name: "Spaces" })).toHaveLength(2);
    expect(screen.queryByRole("link", { name: "About" })).not
      .toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Home" })).not
      .toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Forms" })).not
      .toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Search" })).not
      .toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Settings" })).not
      .toBeInTheDocument();
  });
});
