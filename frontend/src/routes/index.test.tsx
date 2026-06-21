import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import HomeRoute from "./index";

type AuthApiMock = {
  getSession: ReturnType<typeof vi.fn>;
};

const getSessionMock = () => {
  const mock = (globalThis as typeof globalThis & { authApiMock?: AuthApiMock })
    .authApiMock;
  if (!mock) {
    throw new Error("authApiMock is not initialized");
  }
  return mock.getSession;
};

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>
      {props.children}
    </a>
  ),
}));

vi.mock("~/lib/auth-api", () => ({
  authApi: (() => {
    const authApiMock: AuthApiMock = {
      getSession: vi.fn(),
    };
    (globalThis as typeof globalThis & { authApiMock?: AuthApiMock })
      .authApiMock = authApiMock;
    return authApiMock;
  })(),
}));

describe("home route", () => {
  beforeEach(() => {
    localStorage.clear();
    setLocale("en");
    getSessionMock().mockReset();
  });

  it("REQ-E2E-008: public home page routes Learn More to the canonical getting-started docsite flow", async () => {
    getSessionMock().mockResolvedValue({ authenticated: false });

    render(() => <HomeRoute />);

    expect(screen.getByRole("heading", { name: "Ugoite" })).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByRole("link", { name: "Open Spaces" })).toHaveAttribute(
        "href",
        "/login?next=%2Fspaces",
      );
    });
    expect(screen.getByRole("link", { name: "Learn More" })).toHaveAttribute(
      "href",
      "https://ugoite.github.io/ugoite/getting-started",
    );
  });

  it("REQ-OPS-015: public home page points first-time users to /login before /spaces", async () => {
    getSessionMock().mockResolvedValue({ authenticated: false });

    render(() => <HomeRoute />);

    const loginLink = screen.getByRole("link", { name: "Log in" });

    expect(screen.getByRole("heading", { name: "Ugoite" })).toBeInTheDocument();
    expect(loginLink).toHaveAttribute("href", "/login");
    expect(loginLink).toHaveClass("ui-button-primary");
    await waitFor(() => {
      const spacesLink = screen.getByRole("link", { name: "Open Spaces" });
      expect(spacesLink).toHaveAttribute("href", "/login?next=%2Fspaces");
      expect(spacesLink).toHaveClass("ui-button-secondary");
      expect(screen.getByText(/Start with Log in\./i)).toHaveTextContent(
        "/spaces requires an authenticated browser session.",
      );
    });
  });

  it("REQ-OPS-015: authenticated home page keeps Open Spaces on the protected /spaces route", async () => {
    getSessionMock().mockResolvedValue({ authenticated: true });

    render(() => <HomeRoute />);

    await waitFor(() => {
      expect(screen.getByRole("link", { name: "Open Spaces" })).toHaveAttribute(
        "href",
        "/spaces",
      );
    });
  });

  it("REQ-FE-044: localizes home route CTA copy in Japanese", async () => {
    getSessionMock().mockResolvedValue({ authenticated: false });

    render(() => <HomeRoute />);
    setLocale("ja");

    expect(
      screen.getByText(
        "ローカルファーストの知識を、検索と自動化のために構造化",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "ログイン" })).toHaveAttribute(
      "href",
      "/login",
    );
    await waitFor(() => {
      expect(screen.getByRole("link", { name: "スペースを開く" }))
        .toHaveAttribute(
          "href",
          "/login?next=%2Fspaces",
        );
    });
    expect(screen.getByRole("link", { name: "詳しく見る" })).toHaveAttribute(
      "href",
      "https://ugoite.github.io/ugoite/getting-started",
    );
  });
});
