import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import AboutRoute from "./about";
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
describe("concept public pages", () => {
  beforeEach(() => setLocale("en"));
  it("renders About inside the new global shell and localizes it", async () => {
    render(() => <AboutRoute />);
    expect(screen.getByRole("heading", { name: "About Ugoite" }))
      .toBeInTheDocument();
    expect(screen.getAllByText("Ugoite").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Home").length).toBeGreaterThan(0);
    expect(screen.getByRole("link", { name: "Sign in" })).toHaveAttribute(
      "href",
      "/login",
    );
    setLocale("ja");
    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Ugoite について" }))
        .toBeInTheDocument()
    );
    expect(screen.getAllByText("ホーム").length).toBeGreaterThan(0);
  });

  it("REQ-FE-064: public landing pages render the selected locale", async () => {
    setLocale("ja");

    render(() => <IndexRoute />);
    expect(
      screen.getByText("ローカルファーストの知識を、検索と自動化のために構造化"),
    ).toBeInTheDocument();

    cleanup();
    render(() => <AboutRoute />);
    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Ugoite について" }))
        .toBeInTheDocument()
    );
    expect(screen.getByText("ローカルファーストの所有権")).toBeInTheDocument();
  });
});
