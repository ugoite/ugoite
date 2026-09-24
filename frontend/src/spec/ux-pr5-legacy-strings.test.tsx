import "@testing-library/jest-dom/vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
} from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GlobalShell } from "~/components/GlobalShell";
import AboutRedirectRoute from "~/routes/about";
import NotFoundRoute from "~/routes/[...404]";
import SpacesIndexRoute from "~/routes/spaces/index";
import { setLocale } from "~/lib/i18n";
import uiDictionary from "../../../shared/i18n/ui.json";

vi.mock("@solidjs/router", () => ({
  A: (props: Record<string, unknown>) => {
    const { children, ...rest } = props;
    return <a {...(rest as never)}>{children as never}</a>;
  },
  useNavigate: () => vi.fn(),
  useParams: () => ({}),
}));

vi.mock("~/lib/ugoite-client", () => ({
  authApi: {
    clearSession: vi.fn(),
    loginWithPasskey: vi.fn(),
  },
  spaceApi: {
    list: vi.fn(),
    create: vi.fn(),
  },
}));

const legacyLiterals = ["招待で参加", "このアプリについて", "スペースを作成"];

const renderedText = () => document.body.textContent ?? "";

describe("UX PR-5 legacy strings", () => {
  const originalLocation = window.location;
  const replaceMock = vi.fn();

  beforeEach(async () => {
    localStorage.clear();
    replaceMock.mockReset();
    Object.defineProperty(window, "location", {
      configurable: true,
      value: { ...originalLocation, replace: replaceMock },
    });
    const { spaceApi } = await import("~/lib/ugoite-client");
    vi.mocked(spaceApi.list).mockResolvedValue([]);
  });

  it("REQ-UX-I18N-001: forbids legacy Japanese strings on touched surfaces", async () => {
    for (const literal of legacyLiterals) {
      const hits = Object.entries(uiDictionary.ja).filter(([, value]) =>
        value.includes(literal)
      );
      expect(hits, `legacy literal still in JA dictionary: ${literal}`).toEqual(
        [],
      );
    }
    expect(uiDictionary.ja["spacesPage.join"]).toBe("招待コード");
    expect(uiDictionary.ja["account.docs"]).toBe("ドキュメント");

    setLocale("ja");

    render(() => (
      <GlobalShell>
        <p>内容</p>
      </GlobalShell>
    ));
    fireEvent.click(screen.getByRole("button", { name: "アカウント" }));
    for (const literal of legacyLiterals) {
      expect(
        renderedText(),
        `legacy literal rendered by shell/account menu: ${literal}`,
      ).not.toContain(literal);
    }
    expect(screen.getByRole("menuitem", { name: "ドキュメント" }))
      .toBeInTheDocument();

    render(() => <AboutRedirectRoute />);
    render(() => <NotFoundRoute />);
    for (const literal of legacyLiterals) {
      expect(
        renderedText(),
        `legacy literal rendered by redirect/fallback: ${literal}`,
      ).not.toContain(literal);
    }

    render(() => <SpacesIndexRoute />);
    await waitFor(() =>
      expect(screen.getByRole("link", { name: "招待コード" }))
        .toHaveAttribute("href", "/spaces/join")
    );
    expect(screen.getByRole("button", { name: "新しいスペース" }))
      .toBeInTheDocument();
    for (const literal of legacyLiterals) {
      expect(
        renderedText(),
        `legacy literal rendered by spaces list: ${literal}`,
      ).not.toContain(literal);
    }
  });
});
