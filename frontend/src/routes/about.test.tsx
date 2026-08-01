import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@solidjs/testing-library";
import About from "./about";
import { setLocale } from "~/lib/i18n";
import { A } from "@solidjs/router";

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
  useNavigate: () => vi.fn(),
  useParams: () => ({}),
}));

vi.mock("~/lib/ugoite-client", () => ({
  authApi: (() => {
    const authApiMock: AuthApiMock = {
      getSession: vi.fn(),
    };
    (globalThis as typeof globalThis & { authApiMock?: AuthApiMock })
      .authApiMock = authApiMock;
    return authApiMock;
  })(),
}));

describe("/about", () => {
  beforeEach(() => {
    localStorage.clear();
    setLocale("en");
    getSessionMock().mockReset();
  });

  it("REQ-FE-044: localizes about route copy in Japanese", async () => {
    getSessionMock().mockResolvedValue({ authenticated: false });

    render(() => <About />);

    expect(screen.getByText("Rust (Axum)")).toBeInTheDocument();
    setLocale("ja");

    expect(screen.getByRole("heading", { name: "Ugoite について" }))
      .toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByRole("link", { name: "スペースを開く" }))
        .toHaveAttribute(
          "href",
          "/login?next=%2Fspaces",
        );
    });
    expect(screen.getByRole("link", { name: "ホームに戻る" })).toHaveAttribute(
      "href",
      "/",
    );
    expect(screen.getByText(/柔軟な構造と高速な検索/u)).toBeInTheDocument();
    expect(screen.getByText("ローカルファーストの所有権")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "仕組み" })).toBeInTheDocument();
    expect(screen.getByText("技術スタック")).toBeInTheDocument();
    expect(screen.getByText("SolidStart + Tailwind CSS")).toBeInTheDocument();
    expect(screen.getByText("Rust (Axum)")).toBeInTheDocument();
  });

  it("REQ-OPS-015: authenticated about page keeps Open Spaces on /spaces", async () => {
    getSessionMock().mockResolvedValue({ authenticated: true });

    render(() => <About />);

    await waitFor(() => {
      expect(screen.getByRole("link", { name: "Open Spaces" })).toHaveAttribute(
        "href",
        "/spaces",
      );
    });
  });
});
