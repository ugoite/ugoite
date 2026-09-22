import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import AboutRedirectRoute, { aboutDocsHref } from "./about";
import IndexRoute from "./index";

vi.mock("@solidjs/router", () => ({
  A: (props: Record<string, unknown>) => {
    const { children, ...rest } = props;
    return <a {...(rest as never)}>{children as never}</a>;
  },
  useNavigate: () => vi.fn(),
}));

vi.mock(
  "~/lib/ugoite-client",
  () => ({
    authApi: {
      getSession: vi.fn().mockResolvedValue({ authenticated: false }),
    },
  }),
);

const docsHref =
  "https://ugoite.github.io/ugoite/docs/get-started";

describe("concept public pages", () => {
  const originalLocation = window.location;
  const replaceMock = vi.fn();

  beforeEach(() => {
    setLocale("en");
    replaceMock.mockReset();
    Object.defineProperty(window, "location", {
      configurable: true,
      value: { ...originalLocation, replace: replaceMock },
    });
  });

  afterEach(() => {
    Object.defineProperty(window, "location", {
      configurable: true,
      value: originalLocation,
    });
    setLocale("en");
    cleanup();
  });

  it("redirects /about bookmarks to Docs instead of rendering an About page", () => {
    render(() => <AboutRedirectRoute />);

    expect(aboutDocsHref).toBe(docsHref);
    expect(replaceMock).toHaveBeenCalledWith(docsHref);
    expect(screen.getByRole("link", { name: "Docs" })).toHaveAttribute(
      "href",
      docsHref,
    );
    expect(screen.queryByRole("heading", { name: "About Ugoite" })).not
      .toBeInTheDocument();
    expect(screen.queryByText("Spaces")).not.toBeInTheDocument();
  });

  it("REQ-FE-064: public landing pages render the selected locale", async () => {
    setLocale("ja");

    render(() => <IndexRoute />);
    expect(
      screen.getByText(
        "ローカルファーストの知識を、検索と自動化のために構造化",
      ),
    ).toBeInTheDocument();
  });
});
